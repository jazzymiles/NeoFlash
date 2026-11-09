# JSFL Authoring Automation & Adobe JSAPI Compatibility Matrix

NeoFlash provides a deterministic, fuel-bounded, sandboxed **JSFL authoring automation host** (`crates/neoflash-core/src/jsfl.rs`, `T-187`) alongside declarative recorded commands (`AuthoringExtensionManifest`, `T-167`) and native built-in batch tools such as **Smart Batch Rename** (`batch_rename_library_and_instances`).

## Architectural Boundary

- **Authoring-only automation**: JSFL scripts automate the NeoFlash authoring document (`Project`, `Timeline`, `Layer`, `Keyframe`, `Library`, `Element`) at edit time. JSFL is **never** embedded into published HTML5 or SWF packages and is distinct from project/frame/object JavaScript and TypeScript (`neoflash-script`).
- **Atomic single-Undo execution**: Every JSFL run executes against a cloned candidate `Project` and `JsflAuthoringState`. On completion, NeoFlash normalizes motion keyframes, prunes orphaned authoring groups/guides, upgrades format version if needed, and runs `project.validate()`. Any uncaught exception, syntax error, fuel/step timeout, heap/mutation/output limit breach, stale handle access, or validation error rolls back the candidate project, selection, and dirty flag completely and reports an exact `file:line:column` diagnostic (`JsflDiagnostic`).
- **Opaque handle safety**: Scripts receive generation-checked opaque handles (`DocHandle`, `TimelineHandle`, `LayerHandle`, `FrameHandle`, `ItemHandle`, `ElementHandle`) rather than raw Rust pointers. Accessing a handle whose backing layer, keyframe, symbol, or element was deleted raises a located `Stale ...Handle` diagnostic.
- **No ambient OS/network access**: `FLfile` and `fl.runScript` filesystem operations are **denied by default** (`FlFileCapabilities::default()`). When an explicit `allowed_root` and capability flags (`allow_read`, `allow_write`, `allow_delete`) are granted, all `file:///` URIs are canonicalized and checked against `allowed_root`; path traversal (`..`), control characters, remote hosts, and symlinks escaping `allowed_root` are rejected.
- **Native built-in workflows over external utility scripts**: High-frequency production workflows such as **Smart Batch Rename** across Library items and Stage instances (`batch_rename_library_and_instances`) are integrated natively in Rust so users do not need to install or maintain external `.jsfl` utility scripts for them.

---

## Resource Limits (`JsflLimits`)

| Limit | Default | Behavior on Breach |
| --- | --- | --- |
| `max_source_bytes` | `262,144` (256 KiB) | Rejects script before execution with located `JsflDiagnostic`. |
| `max_steps` | `50,000` | Traps immediately (non-catchable by script `try/catch`) and rolls back all mutations. |
| `max_call_depth` | `32` | Traps on deep JS function recursion and rolls back. |
| `max_script_recursion` | `8` | Traps on nested `fl.runScript(...)` cycles and rolls back. |
| `max_heap_nodes` | `16,384` | Traps on excessive Array/Object allocations and rolls back. |
| `max_mutations` | `8,192` | Traps on runaway DOM mutations and rolls back. |
| `max_output_lines` | `1,000` | Traps on `fl.trace` / `outputPanel.trace` flooding and rolls back. |

---

## Official Adobe JSAPI Compatibility Matrix

Status definitions:
- **Working**: Implemented in `crates/neoflash-core/src/jsfl.rs` and proven by automated unit tests.
- **Partial**: Implemented for the representable NeoFlash document model; specific legacy sub-options are noted.
- **Missing**: Outside the current sandboxed delivery; accessing the member produces an explicit located `JsflDiagnostic` error (never a silent no-op).

### 1. Top-Level Global Objects (`fl` / `flash` / `an` & ` outputPanel`)

| Object / Member | Introduced | Status | Notes |
| --- | --- | --- | --- |
| `fl` / `flash` / `an` | Flash MX 2004 / Animate CC | **Working** | All three global aliases resolve to the Flash/Animate authoring host object. |
| `fl.getDocumentDOM()` | Flash MX 2004 | **Working** | Returns `DocHandle` for the active authoring document. |
| `fl.documents` | Flash MX 2004 | **Working** | Array of open `DocHandle`s. |
| `fl.version` | Flash MX 2004 | **Working** | Returns `"NF2 12,0,0,0"`. |
| `fl.configURI` | Flash MX 2004 | **Working** | Returns `"file:///NeoFlash/Configuration/"`. |
| `fl.scriptURI` | CS4 | **Working** | Returns the `file:///` URI of the currently executing script. |
| `fl.trace(...)` | Flash MX 2004 | **Working** | Appends formatted output line to `JsflResult::output_lines` (capped by `max_output_lines`). |
| `fl.outputPanel.trace(...)` | Flash MX 2004 | **Working** | Appends line to Output panel buffer. |
| `fl.outputPanel.clear()` | Flash MX 2004 | **Working** | Clears accumulated Output panel lines. |
| `fl.runScript(fileURI, funcName, ...args)` | Flash MX 2004 | **Working** | Capability-gated (`allow_read` + `allowed_root` or registered virtual script) with `max_script_recursion` guard. |
| `fl.xmlui` / `XMLUI` | Flash MX 2004 | **Missing** | Reports located `Unsupported JSFL property/object` diagnostic. |
| `fl.swfPanels` / `SWFPanel` | CS4 | **Missing** | Reports located `Unsupported JSFL property/object` diagnostic. |
| `CEP` / `CSXS` / `MMExecute` | CS5 / Animate CC | **Missing** | Reports located `Unsupported JSFL function/object` diagnostic. |
| C-level DLL / shared library extensions | Flash MX 2004 | **Missing** | Forbidden by sandbox policy. |

### 2. `Document` Object (`fl.getDocumentDOM()`)

| Member | Introduced | Status | Notes |
| --- | --- | --- | --- |
| `doc.width` / `doc.height` | Flash MX 2004 | **Working** | Get/set stage dimensions (`1..=8192` px). |
| `doc.frameRate` | Flash MX 2004 | **Working** | Get/set document FPS (`1..=240`). |
| `doc.backgroundColor` | Flash MX 2004 | **Working** | Get/set `#RRGGBB` string or `0xRRGGBB` integer. |
| `doc.name` / `doc.docClass` | Flash MX 2004 / CS3 | **Working** | Get/set document name and AS3 document class. |
| `doc.library` | Flash MX 2004 | **Working** | Returns `Library` object for the document. |
| `doc.getTimeline()` | Flash MX 2004 | **Working** | Returns active `TimelineHandle` (scene or edited symbol). |
| `doc.timelines` / `doc.currentTimeline` | Flash MX 2004 | **Working** | Array of scene timelines and active scene index getter/setter. |
| `doc.selection` | Flash MX 2004 | **Working** | Get/set array of `ElementHandle`s for the current selection. |
| `doc.selectAll()` / `doc.selectNone()` | Flash MX 2004 | **Working** | Selects all unlocked visible elements on current frame or clears selection. |
| `doc.group()` / `doc.unGroup()` | Flash MX 2004 | **Working** | Creates or dissolves `AuthoringGroup`s on the active layer. |
| `doc.breakApart()` | Flash MX 2004 | **Working** | Expands `SymbolInstance` children or converts primitives into editable `Path` shapes. |
| `doc.convertToSymbol(type, name, regPoint)` | Flash MX 2004 | **Working** | Creates a `MovieClip`, `Graphic`, or `Button` symbol, adjusts registration offset, and places an instance. |
| `doc.moveSelectionBy({x, y})` | Flash MX 2004 | **Working** | Translates selected elements. |
| `doc.scaleSelection(sx, sy)` | Flash MX 2004 | **Working** | Scales selected elements. |
| `doc.rotateSelection(angle)` | Flash MX 2004 | **Working** | Rotates selected elements in degrees. |
| `doc.deleteSelection()` | Flash MX 2004 | **Working** | Deletes selected elements from the active keyframe. |
| `doc.addNewRectangle(rect, roundness)` | Flash MX 2004 | **Working** | Creates a `Rectangle` element (with optional corner radius) and selects it. |
| `doc.addNewOval(rect)` | Flash MX 2004 | **Working** | Creates an `Ellipse` element and selects it. |
| `doc.addNewText(rect, str)` | Flash MX 2004 | **Working** | Creates a `Text` element and selects it. |
| `doc.addItem(pos, item)` | Flash MX 2004 | **Working** | Instantiates a `Symbol` `ItemHandle` at `{x, y}` on the active keyframe. |
| `doc.exitEditMode()` | Flash MX 2004 | **Working** | Exits symbol edit mode back to `project.root`. |
| `doc.batchRename(options)` | NeoFlash NF2 | **Working** | Invokes native `batch_rename_library_and_instances`. |
| `doc.getProjectColors()` | NeoFlash NF2 | **Working** | Returns array of `{ color, fillCount, strokeCount, gradientCount, textCount, inSwatches, totalUses }` across the entire project (`collect_project_colors`, `T-197`). |
| `doc.changeColor(from, to, [options])` / `doc.replaceColor(...)` | NeoFlash NF2 | **Working** | Replaces `from` with `to` across all scenes, Library symbols, solid fills, strokes, linear/radial/freeform/mesh gradients, text elements, appearance stacks, and custom/global swatches in one validated Undo step (`change_project_color`, `CHANGE_COLOR_JSFL_SCRIPT`, `T-197`). |

### 3. `Timeline`, `Layer`, and `Frame` Objects

| Member | Introduced | Status | Notes |
| --- | --- | --- | --- |
| `timeline.name` | Flash MX 2004 | **Working** | Get/set scene or symbol name. |
| `timeline.currentFrame` / `currentLayer` | Flash MX 2004 | **Working** | Get/set 0-based active frame and layer indices. |
| `timeline.frameCount` / `layerCount` | Flash MX 2004 | **Working** | Read-only frame and layer counts. |
| `timeline.layers` | Flash MX 2004 | **Working** | Returns array of `LayerHandle`s. |
| `timeline.getSelectedLayers()` / `setSelectedLayers(idx, bReplace)` | Flash MX 2004 | **Working** | Reads or updates selected layer indices. |
| `timeline.getSelectedFrames()` / `setSelectedFrames(...)` | Flash MX 2004 | **Working** | Reads or updates `[layerIndex, startFrame, endFrame]` triplets. |
| `timeline.addNewLayer(name, layerType)` | Flash MX 2004 | **Working** | Adds `"normal"`, `"guide"`, `"mask"`, or `"folder"` layer. |
| `timeline.deleteLayer(index)` | Flash MX 2004 | **Working** | Deletes layer (preserving at least one layer) and prunes motion guides. |
| `timeline.reorderLayer(from, to, bAddBefore)` | Flash MX 2004 | **Working** | Reorders layers in `timeline.layers`. |
| `timeline.insertFrames(num, bAllLayers, frameIdx)` | Flash MX 2004 | **Working** | Extends timeline exposure and shifts later keyframes. |
| `timeline.removeFrames(startFrame, endFrame)` | Flash MX 2004 | **Working** | Removes frame span and shifts later keyframes left. |
| `timeline.insertKeyframe(frameIdx)` | Flash MX 2004 | **Working** | Inserts a sampled keyframe at `frameIdx`. |
| `timeline.insertBlankKeyframe(frameIdx)` | Flash MX 2004 | **Working** | Inserts an empty keyframe at `frameIdx`. |
| `timeline.clearKeyframes(startFrame, endFrame)` | Flash MX 2004 | **Working** | Clears non-zero keyframes in `[startFrame..endFrame)`. |
| `timeline.createMotionTween(startFrame, endFrame)` | Flash MX 2004 | **Working** | Enables tweening on keyframes in `[startFrame..endFrame)`. |
| `timeline.reverseFrames(startFrame, endFrame)` | Flash MX 2004 | **Working** | Reverses keyframe timing across `[startFrame..endFrame)`. |
| `layer.name` / `visible` / `locked` / `outline` / `color` | Flash MX 2004 | **Working** | Get/set layer properties. |
| `layer.layerType` | Flash MX 2004 | **Working** | Get/set `"normal"`, `"guide"`, `"guided"`, `"mask"`, `"masked"`, `"folder"`. |
| `layer.frameCount` / `layer.frames` | Flash MX 2004 | **Working** | Returns array of `FrameHandle`s across `0..frameCount`. |
| `frame.name` / `frame.labelType` | Flash MX 2004 | **Working** | Get/set keyframe label and `"none"` / `"name"` / `"comment"`. |
| `frame.actionScript` | Flash MX 2004 | **Working** | Get/set frame script source at the keyframe's start frame. |
| `frame.tweenType` / `frame.tweenEasing` | Flash MX 2004 | **Working** | Get/set `"none"`, `"motion"`, `"classic"`, `"shape"` and Flash acceleration `-100..=100`. |
| `frame.startFrame` / `frame.duration` | Flash MX 2004 | **Working** | Read-only span start frame and duration. |
| `frame.elements` | Flash MX 2004 | **Working** | Array of `ElementHandle`s on the keyframe covering this frame. |

### 4. `Library` and `Item` Objects

| Member | Introduced | Status | Notes |
| --- | --- | --- | --- |
| `library.items` | Flash MX 2004 | **Working** | Array of `ItemHandle`s for folders and symbols. |
| `library.itemExists(namePath)` | Flash MX 2004 | **Working** | Returns `true` if symbol or folder path exists. |
| `library.findItemIndex(namePath)` | Flash MX 2004 | **Working** | Returns index in `library.items` or `-1`. |
| `library.addNewItem(type, namePath)` | Flash MX 2004 | **Working** | Creates `"movie clip"`, `"graphic"`, `"button"`, or `"folder"`. |
| `library.newFolder(folderPath)` | Flash MX 2004 | **Working** | Creates logical library folder path. |
| `library.selectItem(namePath, bReplace)` / `getSelectedItems()` | Flash MX 2004 | **Working** | Selects or queries selected `ItemHandle`s. |
| `library.renameItem(newName)` | Flash MX 2004 | **Working** | Renames selected or targeted Library symbol. |
| `library.moveToFolder(folderPath, itemPath, bReplace)` | Flash MX 2004 | **Working** | Moves symbol into `folderPath`. |
| `library.duplicateItem(namePath)` | Flash MX 2004 | **Working** | Clones symbol and its timeline with fresh IDs. |
| `library.deleteItem(namePath)` | Flash MX 2004 | **Working** | Deletes symbol or folder and purges stage instances of deleted symbols. |
| `library.editItem(namePath)` | Flash MX 2004 | **Working** | Enters symbol-editing mode for `namePath` (or exits to root when omitted). |
| `library.addItemToDocument({x, y}, namePath)` | Flash MX 2004 | **Working** | Places an instance of `namePath` at `{x, y}` on the active keyframe. |
| `library.batchRename(options)` | NeoFlash NF2 | **Working** | Built-in Smart Batch Rename across Library symbols and matching instances. |
| `item.name` / `item.itemType` / `item.timeline` | Flash MX 2004 | **Working** | Get/set item name; read `itemType` and symbol `TimelineHandle`. |
| `item.linkageIdentifier` / `linkageClassName` / `linkageExportForAS` | Flash MX 2004 / CS3 | **Working** | Get/set runtime linkage and AS3 class bindings. |

### 5. `Element`, `Shape`, `SymbolInstance`, `Text`, `Matrix`, `Fill`, and `Stroke`

| Member | Introduced | Status | Notes |
| --- | --- | --- | --- |
| `element.elementType` | Flash MX 2004 | **Working** | Returns `"shape"`, `"shapeObj"`, `"instance"`, or `"text"`. |
| `element.name` | Flash MX 2004 | **Working** | Get/set instance or element name. |
| `element.x` / `y` / `left` / `top` / `width` / `height` | Flash MX 2004 | **Working** | Get/set position and dimensions. |
| `element.scaleX` / `scaleY` / `rotation` / `skewX` / `skewY` | Flash MX 2004 | **Working** | Get/set transform components. |
| `element.matrix` | Flash MX 2004 | **Working** | Get/set `{a, b, c, d, tx, ty}` affine matrix via `Transform::from_matrix`. |
| `element.visible` / `element.blendMode` | Flash 8 | **Working** | Get/set visibility and blend mode. |
| `instance.libraryItem` / `symbolType` | Flash MX 2004 | **Working** | Get/set linked `ItemHandle` (swap symbol) and read symbol kind. |
| `instance.firstFrame` / `instance.loop` | Flash MX 2004 | **Working** | Get/set graphic playback mode (`"loop"`, `"play once"`, `"single frame"`) and 0-based `firstFrame`. |
| `text.getTextString()` / `text.setTextString(str)` | Flash MX 2004 | **Working** | Reads or mutates text content on `Text` and `TextBox` elements. |
| `text.getTextAttr(attr)` / `text.setTextAttr(attr, val)` | Flash MX 2004 | **Working** | Reads or mutates `"fillColor"` / `"color"`, `"size"`, and `"face"` on `Text` and `TextBox` elements (`T-197`). |
| `element.fillColor` / `strokeColor` / `color` | NeoFlash NF2 | **Working** | Direct `#RRGGBB` / `#RRGGBBAA` getter/setter for shape fills, strokes, and text colours (`T-197`). |
| `shape.getCustomFill()` / `setCustomFill(fill)` | Flash MX 2004 | **Working** | Supports `"solid"`, `"noFill"`, `"linearGradient"`, and `"radialGradient"` with `#RRGGBB` / `#RRGGBBAA` `color`, `colorArray`, and `posArray` (`T-197`). |
| `shape.getCustomStroke()` / `setCustomStroke(stroke)` | Flash MX 2004 | **Working** | Supports `"solid"`, `"noStroke"`, `"linearGradient"`, and `"radialGradient"` with `color`, `thickness`, `colorArray`, and `posArray` (`T-197`). |
| `shape.contours` / `edges` / `vertices` / `isGroup` | Flash MX 2004 | **Partial** | Read-only vertex/command inspection and group/Object-Drawing flag. |

### 6. Capability-Gated `FLfile` Object

| Member | Introduced | Status | Notes |
| --- | --- | --- | --- |
| `FLfile.platformPathToURI(path)` | Flash MX 2004 | **Working** | Pure path-to-`file:///` conversion; rejects control characters. |
| `FLfile.uriToPlatformPath(fileURI)` | Flash MX 2004 | **Working** | Pure `file:///`-to-path conversion; rejects non-`file:///` schemes. |
| `FLfile.exists(fileURI)` | Flash MX 2004 | **Working** | Requires `allow_read` + canonical containment inside `allowed_root`. |
| `FLfile.read(fileURI)` | Flash MX 2004 | **Working** | Requires `allow_read` + canonical containment inside `allowed_root`. |
| `FLfile.write(fileURI, text, mode)` | Flash MX 2004 | **Working** | Requires `allow_write` + canonical containment inside `allowed_root` (supports `"append"`). |
| `FLfile.createFolder(folderURI)` | Flash MX 2004 | **Working** | Requires `allow_write` + canonical containment inside `allowed_root`. |
| `FLfile.remove(fileOrFolderURI)` | Flash MX 2004 | **Working** | Requires `allow_delete` + canonical containment inside `allowed_root`. |
| `FLfile.listFolder(folderURI, bFilesOnly)` | Flash MX 2004 | **Working** | Requires `allow_read` + canonical containment inside `allowed_root`; returns sorted filenames. |
