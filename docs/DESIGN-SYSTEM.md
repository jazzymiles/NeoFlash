# NeoFlash UI design system

This is the **target standard** for NeoFlash authoring UI. It does not claim that every rule is implemented or visually accepted.
New panels, dialogs, controls and editor-owned preview chrome MUST follow this system. Existing UI migrates through the shared components below.
Authored artwork and published project content retain their own colors, typography and behavior.

The machine-readable values live in [tokens.json](design-system/tokens.json); the [reference preview](design-system/preview.html) illustrates their use.
This document defines behavior and composition. Keep the document, tokens and preview consistent when changing the standard.
Current workflows and implementation boundaries remain in [Workspace](WORKSPACE.md) and [Verification](VERIFICATION.md).
**MUST** is a requirement. Optional behavior is permitted only as an explicitly documented system variant. There are no panel-specific visual exceptions: an uncovered case must be resolved in this system and the shared component before shipping.

## Principles

- Keep chrome quiet so artwork, code and timeline structure carry the visual emphasis.
- Use compact aligned data, stable control locations and a clear current context.
- Show the operation's target before its settings; do not make users infer it from selection color alone.
- Keep common actions visible and put secondary actions in predictable menus.
- Preserve the same meaning and editing behavior across themes, narrow layouts and detached windows.
- Improve classical workflows without reproducing historical icons or incidental visual defects.

The [Adobe Flash CS3 manual](https://help.adobe.com/archive/en_US/flash/cs3/flash_cs3_help.pdf), printed pages 41–42 on Library and Actions, informs these principles: persistent work areas, contextual controls and dense organization. NeoFlash's icons and component system remain its own.

The visual references were inspected as screenshots embedded in manuals or author-published tutorials, not live legacy applications. The dimensions below are NeoFlash design decisions, not measurements copied from scaled screenshots.

| Reference | Principle retained |
| --- | --- |
| [Flash MX 2004, p. 24](https://download.macromedia.com/pub/documentation/en/flash/mx2004/fl_getting_started.pdf#page=24) | One aligned layer/frame table, stable state columns and a compact transport strip. |
| [Flash 8, p. 5](https://gct.com.au/SampleFiles/Flash8/Flash_Ch4.pdf#page=5), Greg Bowden / Guided Computer Tutorials | Separate scene context, ruler, content and transport. |
| [Flash CS3, pp. 34, 41–42](https://help.adobe.com/archive/en_US/flash/cs3/flash_cs3_help.pdf#page=40) | Distinct active layer, playhead and tween; Library preview/list/actions; explicit Actions target. |
| [Flash CS6, p. 2](https://gct.com.au/SampleFiles/flash_cs6/FlashCS6_Ch1.pdf#page=2), Greg Bowden / Guided Computer Tutorials | Quiet dock groups, thin tabs and a dominant Stage. Panel position is configurable, not a fixed aesthetic rule. |

Do not copy unreadably small legacy targets, ambiguous icon-only commands, color-only encoding or rigid panel placement. No Adobe artwork or icon assets are distributed with this reference.

## Units, density and typography

All dimensions below are **logical points**, independent of physical pixels and display scale.
Use semantic tokens rather than local font, spacing or color literals. Optical geometry may use derived coordinates.

| Role | Target |
| --- | --- |
| Body / control text | 12 pt; 16 pt line height |
| Secondary text | 11 pt; never the only carrier of essential instructions |
| Panel title / section title | 12 pt, medium weight |
| Code | 12 pt monospace; 18 pt line height |
| Spacing scale | 4, 8, 12, 16 pt |
| Standard control / minimum icon hit area | 24 pt |
| Property row | 28 pt |
| List row / timeline row / timeline ruler | 24 pt |
| Panel header / panel footer | 28 pt |
| Section disclosure header | 24 pt |
| Standard panel content padding | 8 pt |
| Standard icon / icon stroke | 16 pt / 1.5 pt |
| Corner radius / divider / focus stroke | 3 pt / 1 pt / 2 pt |

Use spacing and weight before adding boxes or decorative surfaces. Avoid a border around every passive label.
Larger accessibility targets MAY be provided without enlarging icon geometry or changing command meaning.
Do not shrink text, hit areas or line height to conceal an overflow problem.
Pixel snapping MUST preserve these logical dimensions on Retina and other display scales.
Stage selection/transform overlays use a 1 pt outline with a 3 pt halo behind it. The outline/halo pair has at least 9:1 contrast between its colors, so one member reaches at least 3:1 against a uniform authored background luminance. Verify the pair across the grayscale and representative artwork; never recolor authored content to improve editor contrast. Handles remain 6 pt in screen space.

## Color, hierarchy and states

Light and Dark use the same semantic roles from the token file: surfaces, text, muted text, borders, selection, focus, error and warning.
Timeline, pasteboard and stage overlays MUST also use these roles; authored colors MUST remain unchanged by theme switching.
Normal text MUST reach 4.5:1 contrast against its actual surface; meaningful control boundaries and focus indicators MUST reach 3:1.
Check composed states and overlays, not only isolated palette swatches. Timeline grid reaches at least 2.5:1 and `timelineDivider` reaches at least 3:1 against normal, major, tween and selected timeline surfaces. The generic structural `divider` is not a substitute for this timeline role. Never use hue alone to convey a state.

| State | Required presentation and behavior |
| --- | --- |
| Normal | Readable label and recognizable interaction boundary where needed. |
| Hover | Subtle surface or border change; no geometry shift. |
| Pressed | Immediate feedback during activation; distinct from persistent selection. |
| Selected | Persistent marker plus shape, border or label cue where ambiguity is possible. |
| Focus | Visible 2 pt focus indicator, additive to selection and error states. |
| Disabled | Clearly unavailable, with the reason available through help and accessibility; no misleading activation. |
| Error | Icon/text and affected field or source location; preserve recoverable user input. |
| Warning | Explain the consequence and available action without blocking unrelated work. |
| Mixed | Explicit mixed-value presentation; never substitute an arbitrary selected object's value. |

Panel focus, selected artwork and selected tabs are different meanings and MUST remain distinguishable.
Theme changes MUST preserve focus, scroll, selection, active edits and authored history.

## Shared controls and fields

Every field MUST have a visible label or an unambiguous nearby context plus a full accessible name.
Property labels use a 76 pt column, with 8 pt between label and value and 8 pt content padding.
Long labels MUST retain their full meaning through disclosure or tooltip and accessibility; silent truncation is insufficient.
**Paired fields / compact variant:** X/Y and W/H use short visible labels with full accessible names and units. Each field reserves 12 pt for its label, a 4 pt label/control gap and at least 64 pt for its control; the pair gap is 8 pt. The baseline minimum is **168 pt**: `2 × (12 + 4 + 64) + 8`. Measure actual remaining content width after panel padding, disclosure indentation and any reserved non-overlay scrollbar strip. A standard **280 pt inspector MUST show these pairs side by side**. Use one column only when that remaining width is below the measured pair minimum. Longer labels, units or values increase the measured minimum and require an adaptive layout; never clip them or shrink text/targets. Both layouts retain the same input semantics.

- Numeric fields support click-to-type and drag-to-scrub with units and appropriate precision.
- Enter or blur applies typed values; Escape cancels the pending edit.
- A continuous drag is one Undo operation. Invalid values explain the limit and preserve the last valid document state.
- Wide and narrow layouts use the same control implementation and commit rules.
- Text, number, choice, color and mixed-value fields share label, help, error and reset placement.
- Color controls distinguish RGB from alpha; changing transparency MUST preserve the intended RGB value.
- A disabled control explains prerequisites, such as selecting two objects, rather than relying on dimming alone.

Icons MUST keep one meaning across action, tooltip, accessible name and shortcut.
Use vector geometry on the standard grid; do not substitute Unicode letters such as Q/P for tool icons.
Show/Hide and Lock/Unlock tooltips and accessible actions name the **next action**; the visible state indicator describes the current state.
Icon-only controls MUST have keyboard focus, an accessible name and a tooltip. Larger targets MAY surround the same 16 pt glyph.

## Workspace and panel shell

Stage has priority. The useful stage-area target is at least **320 × 240 pt**, excluding surrounding editor chrome.
At **1024 × 720**, Library and Properties share a single right-side tab group by default. Use this compact default at widths up to 1120 pt or heights up to 800 pt; do not override a deliberate saved layout on every resize.
At **1440 × 960**, Library above Properties is the default split. User customization remains persistent.
Do not consume the stage with redundant toolbars, empty status strips or permanently expanded help.
Use a footer only for stable panel actions or status; do not add an extra bar for every feature.

Every panel uses one shared shell: 28 pt header, content region and optional 28 pt footer.
Tabs, panel menu, group-drag grip and resize grip MUST be visually distinct and keyboard reachable where applicable.
Overflow MUST have a discoverable continuation or overflow control; hidden scrolling alone is insufficient.
Keep headers and essential actions fixed while the relevant content region scrolls independently.

**Flat tabs / shared variant:** use a 28 pt row, 8 pt horizontal padding per tab and 4 pt between tabs. Labels use 12 pt body text at normal weight 400 in every state; selection MUST NOT change their weight. Inactive tabs remain quiet and readable; hover adds the semantic hover treatment without movement. Selection has a persistent **2 pt bottom marker and 1 pt boundary**, both using the semantic `selectionStroke` role (`selection_stroke` in native UI), never hue alone. The 2 pt focus indicator remains distinct and additive to selection. Every tab exposes its full accessible name, tab role and selected state; keyboard navigation MUST reach, activate and leave the tab group, including overflowed tabs.

**Scrollbar / shared variant:** reserve a 12 pt strip with an 8 pt thumb centered by 2 pt insets and a minimum thumb length of 24 pt. Track, normal thumb, hover, drag and focus use dedicated semantic muted-gray roles, never the main-text token. The thumb MUST contrast at least 3:1 against its track in every state in both themes; focus remains distinguishable without relying on hue. The strip never overlays content or interactive targets, and hover/drag MUST NOT change the space reserved for it.

All panels MUST support true native detachment through the same shell and shared content implementation.
Detaching MUST move the existing view, not create another document or divergent panel state.
Dock, close/reopen and layout restoration preserve target, selection, active edits and scroll where meaningful.
Recover offscreen windows without repeatedly fighting deliberate window movement.
Workspace preferences MUST remain separate from authored data and MUST NOT mark the document dirty.

## Properties inspector

Properties has a panel header, a concise context header and disclosure sections.
Document, Frame and Object targets follow explicit authoring context automatically; the target is always visible.
Do not silently replace an Object target with tool defaults merely because a drawing tool is selected.
**Tool** is an explicit inspector mode; entering it MUST NOT change the authored selection or current object/frame target.
Returning from Tool mode restores the prior contextual inspector.

| Context | Section order |
| --- | --- |
| Object | Identity → Transform → Appearance → Behavior / linkage → Accessibility |
| Frame | Range / duration → Tween → Label / script |
| Document | Stage → Playback → Publish |
| Tool | Tool identity → Relevant options → Drawing defaults |

Section headers are 24 pt with a disclosure control and clear text.
Remember disclosure state by context, not as one global state that hides unrelated sections.
Keep essential identity and geometry readily available; advanced options MAY start collapsed.
For multiple selection, show the count and mixed values; applying a value changes only the named property.
Frame settings MUST distinguish authored duration from viewport zoom or playback observation.
Object and tool paint controls MUST state which target they modify.

## Library

Library composition is: **header 28 → search 24 → optional preview → list → footer 28**.
The preview is collapsible, defaults to 96 pt and may resize within 64–160 pt.
No selected symbol means the preview is hidden; an empty placeholder MUST NOT reserve its full height.
Preview does not autoplay. Any playback action is explicit and MUST NOT alter the document playhead.
The list scrolls independently while search and footer actions remain reachable.
Rows are 24 pt; prioritize Name, with Type optional when width permits. Secondary metadata must not crowd out names.
Folder hierarchy and selected state MUST remain legible without relying on color alone.
Search/filter state MUST be visible and easy to clear.

**Library / previewOverflow variant:** reserve the measured header, search, sort and footer heights, their spacing and at least three list rows (**72 pt**) before calculating the preview budget. When the remaining height accommodates a **24 pt preview header + 64 pt minimum preview + measured metadata**, show the user's open preview inline at **64–160 pt**, within that budget. Otherwise retain a **24 pt Preview row** with an explicit opening glyph and the accessible action **Open symbol preview**. Activation opens a bounded popover anchored to that row, using the same renderer and preview state, without autoplay. Escape or an outside click closes it and returns focus to the row. Preserve the user's preview-open preference and chosen preview height across layout changes; automatically restore inline presentation when space permits and the preference is open. Neither the compact row nor its popover may create horizontal overflow in a small panel or exceed the usable viewport.

The footer exposes **New Symbol**, **New Folder** and **Import**; **Delete** is a separate destructive action.
Use concise icons with full names when labels do not fit. Do not combine destructive and creation actions ambiguously.
Distinguish importing/placing graphics from merely adding a stored resource.
Empty, no-result and unavailable states provide the next useful action without displacing the list unnecessarily.

## Tools

The compact Tools rail is **64 pt** wide, with two centered columns of 24 pt targets and a 4 pt gap.
The rail is a compact shell variant; do not inherit inspector padding that makes this grid overflow.
Organize tools into three visible semantic groups: **Selection**, **Drawing**, **Navigation**.
Keep group and tool locations stable as selection changes; indicate the active tool clearly.
Long settings belong in the explicit Tool inspector instead of extending a narrow vertical toolbar indefinitely.
Fill and stroke controls sit at the bottom and remain reachable in supported window sizes.
**Tools rail / scroll variant:** 64 pt is the content width; the 6 pt docking resize target is outside it. When the tool list overflows vertically, reserve a separate 12 pt scroll strip, making the content width 76 pt (82 pt including an applicable dock splitter). The 24/4/24 grid and glyph size do not change. Reserve this strip for the whole overflow state, not only on hover; do not cover tools with an overlay scrollbar or switch to pagination. A detached Tools window must accommodate this minimum. Bottom paint/actions remain fixed; only the list scrolls.
Tool changes and temporary Hand navigation MUST preserve the user's authored selection and unfinished edit according to the tool contract.

## Timeline

Timeline defaults to **200 pt** high with a **160 pt** minimum; user resizing remains available.
Rows and ruler are 24 pt; the footer is 28 pt. Avoid adding another permanent row for each new timeline feature.
The layer-name region defaults to 200 pt and resizes within 140–320 pt.
Layer labels, status controls and frame cells MUST share the same row positions during vertical scrolling.
Frame zoom changes horizontal cell width only; stage zoom and timeline zoom have separate labels and controls.
Frame markers and strokes remain visually stable. Tween arrowheads use fixed **6 × 3 pt** geometry and stay clipped to their row.
Key, blank key, held content, tween, script, label, mask, guide and selection MUST not depend on hue alone.
The playhead, current frame and actual timeline end remain identifiable at every supported zoom.
Scrollbars occupy their own strips rather than covering editable frame cells.
Layer operations, playback and onion controls have consistent grouping and accessible names.
Dense views may simplify secondary detail, but MUST preserve essential markers and clear access to hidden controls.

## Actions and diagnostics

Use one **28 pt target-and-language row** identifying symbol/frame/object/source and language.
The code area uses 12 pt monospace text with an 18 pt line height and receives the remaining useful space.
Secondary commands belong in the panel menu or a compact action region, not multiple permanent header rows.
Diagnostics identify the source and line/column, mark the affected source and provide a concise inline diagnostic row.
Keep diagnostics associated with their source; switching target MUST NOT show an unrelated stale result.
F9 reveals Actions without replacing text or target, and preserves caret, selection and scroll for that target.
An explicit frame/object command may choose a new target; its header MUST immediately show that choice.
Docking and detaching preserve the same editor state and accessible text interaction.

## Menus and command language

Use one command definition for meaning, label, icon, shortcut, enabled state and disabled reason across menus and panels.
Align shortcuts in a separate column; do not emulate alignment with spaces in labels.
Use full consistent names, conventional ellipses for commands requiring more input, and explicit destructive wording.
Alignment and distribution MUST use distinct symbols/help even where both involve left, center or right edges.
Menus, dialogs and popovers follow the same typography, spacing, states and focus rules as panels.

All top-level, context and panel menus use the same menu-row component: 24 pt height, 8 pt horizontal inset and at least 16 pt between the label and shortcut. Check/icon, label, shortcut and submenu arrow have separate measured columns. Width comes from these columns, never the parent workspace's available width or a stretching right-to-left layout. Ordinary popups use 220–320 pt; the long-label variant grows only to its measured content and remains within the screen's usable width minus 16 pt. Height is capped by the usable screen minus 16 pt, with a scrollable body and visible continuation when necessary. These are system variants, not individual menu overrides.

Window shows panel visibility once, then **Detach Panel** and **Dock Panel** submenus containing the applicable panels, **Restore Workspace**, **Appearance** (Light/Dark) and **About NeoFlash**. Native dock/undock remains available from each panel's common menu. Do not repeat all panels as a second flat list of docking commands.

Keyboard chords use one renderer and accessible names. A missing modifier glyph uses the named modifier (for example, Shift) through the shared renderer; missing-glyph squares are never an accepted fallback. Menu focus, keyboard navigation, submenu positioning and focus return are shared behavior. Disabled explanations remain discoverable without enabling an unavailable action.

## Dialog and mixed-value variants

**Symbol dialog / registration grid:** use the common 76 pt label column and 8 pt gap. The nine registration positions form a 3 × 3 grid of 24 pt targets with 4 pt gaps. Each target has a full position name, persistent selected indication and the common focus treatment. Keyboard navigation must reach and choose every position. A right-aligned action row uses 24 pt buttons, Cancel followed by the specific primary action (Create or Convert). Validation stays near its field and preserves the entered values; cancel returns focus without authoring changes.

**Color / mixed selection:** Fill, Stroke and Width independently show Mixed when selected editable objects disagree. Never show one object's value as the combined value. A field's explicit draft and Apply operation changes only that field across the selected objects, as one Undo operation. Other mixed fields remain untouched; choosing a fill must not overwrite stroke or width. Mixed, None/transparent and a concrete value are distinct presentations. Apply remains unavailable until that field has a valid deliberate draft. The panel states the affected selection and excludes non-editable targets according to the shared selection contract.

## Mandatory contract for future UI

Before introducing a panel or control, define its target, primary operation, empty state and selection/mixed-value behavior.
Specify narrow layout, overflow, keyboard order, accessible names, focus, errors, Undo and persistence boundaries.
Use existing tokens and components; a new visual role requires an update to this standard, tokens and reference preview.
A recurring case is promoted into a named component variant with its trigger, geometry, states and behavior. A case encountered once still uses an existing pattern or receives a defined pattern before implementation. Do not accumulate temporary per-panel styling patches.
The same rule covers menus, tooltips, color pickers, numeric fields, tree/list rows, empty/error states, dialogs, code controls and manually drawn overlays. Each delivery records coverage of every affected surface; “most controls migrated” does not establish compliance for the remaining controls.
New panels MUST work docked and detached without duplicated state or a second custom shell.
Keep product wording independent of implementation details unless those details help users make a decision.

Migrate through [theme.rs](../crates/neoflash-editor/src/theme.rs), [property_row / compact_number](../crates/neoflash-editor/src/paint_controls.rs), [IconButton](../crates/neoflash-editor/src/ui_icons.rs) and [dock_group](../crates/neoflash-editor/src/dock_grid.rs).
Manual timeline/stage painters consume the same roles. Migrate related controls together rather than adding a parallel styling system.

## Shared acceptance matrix

Validate coherent UI changes in one representative delivery pass; repeat only a concrete affected failure.

| Scenario | Required checks |
| --- | --- |
| 1024 × 720 and 1440 × 960, Light and Dark | Stage priority, correct default right-side composition, legible content, no clipped essential controls. |
| Keyboard and focus | Tab order, visible focus, shortcuts, numeric Enter/Escape/blur, mixed values and disabled explanations. |
| Dense and empty content | Long names, many layers/symbols, narrow inspectors, overflow disclosure and independent scrolling. |
| Library preview overflow | Shrink/grow across the measured threshold while preserving three list rows, preview preference and height; open the compact Preview row by keyboard, close with Escape/outside click with focus return, and restore inline preview without autoplay or horizontal overflow. |
| Standard 280 pt inspector | X/Y and W/H remain paired after padding, disclosure indentation and a reserved scrollbar strip; longer content adapts without clipping, with full accessible names and unchanged editing behavior. |
| Flat tabs, pointer and keyboard | Labels remain 12 pt at normal weight 400; selection has a persistent 2 pt bottom marker and 1 pt boundary in `selectionStroke`. Hover does not shift layout, the additive 2 pt focus remains distinct, full names and selected state are exposed, and overflowed tabs remain reachable. |
| Scrollbars, Light and Dark | 12 pt strip, 8 pt thumb, 2 pt insets and 24 pt minimum length; thumb/track contrast ≥3:1 in normal, hover, drag and focus, with no content overlay or geometry jump. |
| Menus and dialogs | Shortcut alignment, clear action names, safe cancellation, focus return and reachable primary actions. |
| Detach → move/resize → dock → reopen | One shared state, preserved edits/caret/scroll, offscreen recovery and no authored dirty change. |
| Retina / supported scaling | Stable logical sizes, crisp icons, fixed marker geometry and readable boundaries. |

Record which live interactions and themes were actually checked. Source inspection or the reference preview alone is not native UI acceptance.

## Using the reference

From the repository root, serve the documentation locally:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory docs
```

Open `http://127.0.0.1:8765/design-system/preview.html`. The page loads the canonical JSON tokens; it has no external dependencies. Workspace and Components show both themes, contextual fields, library search, timeline markers, panel visibility, an Actions editor and a dialog with error/focus states.

The specimen uses disposable sample data. It does not compile scripts, import files, implement native windows or edit a NeoFlash document. Numeric dragging, full shortcut routing, Undo, native detachment, DPI behavior and assistive-technology acceptance remain implementation checks. The source documents the reference, not a second authoring application.
