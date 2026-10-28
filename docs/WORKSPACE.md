# Workspace

NeoFlash uses a compact classic workspace. **Window → Appearance → Light / Dark** changes both ordinary controls and manually painted timeline/stage overlays. Authored colors, fonts, scripts and document history are independent of the theme.

## Panels

Window controls Timeline, Tools, Library, Properties, Actions, Accessibility and Output. A hidden panel can be shown again from the same menu. **F9** reveals Actions and focuses its editor even while typing in another field. Selecting a frame first makes it the Actions target; the timeline's context menu and **Actions · F9** button also open that frame directly. The header identifies the current symbol and frame. Opening Actions pauses preview playback.

All seven panels support **Window → Detach / Dock** and header dragging. Drag a panel title or tab to any of the four highlighted edges, to a tab group, or to a group's split target. Top/bottom areas split horizontally; left/right areas split vertically. Resize areas at their outside boundaries and groups at their dividers. Drag outside the targets to open a native floating window. The default right side shows Library above Properties. The macOS title bar moves a window; its internal header performs docking.

**Dock** also returns a floating panel to the main window. Closing a detached window hides it and retains its placement so Window or F9 can reopen it. There is one document, selection, playhead and script editor across these windows. Actions retains the text cursor when moved between its dock and native window; native text shortcuts and the detached Edit menu address that text field.

**Restore Workspace** returns all panels to the default docked layout and sizes. It preserves the chosen theme, authored document, selection, playhead and unfinished drawing. Layout/theme preferences are saved separately by the application; saving a project does not include machine-specific window positions. On macOS, disconnected-display recovery uses the usable areas of all connected screens and moves inaccessible panel title bars back into reach.

Recursive splits inside groups, named workspace presets and multiple open documents remain planned. Native menus, VoiceOver and IME need broader platform acceptance.

## Timeline and compact windows

Layer names and frame cells share one vertical scroll body, with a fixed ruler. Frames scroll horizontally within that body. Tools and Library scroll vertically when space is limited; document properties and onion controls wrap. Tween arrows have a fixed six-point arrowhead clipped to their own row, independent of their span length. Icons use vector geometry instead of relying on installed symbol glyphs.

Layer names use compact left-aligned text, with row dividers through the name and property columns. Double-click a name or choose **Rename layer** in its context menu. Enter or leaving the field commits a nonempty name as one Undo step; Escape cancels. **Frame zoom**, − / + and **Fit** change frame-cell width with adaptive ruler labels and keep the selected frame in view. They do not change authored frames, FPS or duration.

Scrollbars occupy separate strips outside the frame cells. Held content and blank-key intervals extend visibly to the next key or symbol end; the **End** marker separates actual frames from unused space. A key drag previews its destination and scrolls at the horizontal edge. Occupied destinations are refused. Per-layer lengths and frame-range selection/clipboard remain planned; use a blank key to end a layer's visible content. Active onion skin is identified in the stage header because neighbouring-frame ghosts can appear over an otherwise blank frame.

The minimum main window is 1024 × 720 logical points. Rendering and interaction regressions cover this size and larger windows, both themes, long libraries, many layers and long spans. See [the verification record](VERIFICATION.md) for the separately observed native checks and their limits.

## Creating symbols

**F8 / Convert to Symbol** offers a name, Movie Clip / Graphic / Button type and nine registration points. Conversion preserves the artwork's placement and commits as one Undo step. Multiple objects must form a contiguous stack on one layer so grouping does not reorder unrelated artwork. New symbols start with one frame. Movie Clips can export a runtime identifier and bind an existing class from Project files; the class source defines its base class. Buttons currently use a single-frame/event profile; Up/Over/Down/Hit authoring remains unfinished. 9-slice guides and legacy SWF library sharing are not implemented.


## Layer roles and operations

Right-click a layer for visibility/locking, insert/delete/rename, copy/cut/paste/duplicate, merge, symbol conversion and folder operations. A folder groups its descendants; hidden or locked folders affect them. Layer Properties selects a parent folder or mask source. Merge with Next Layer preserves animation, sampling tweened spans into keys when necessary. Layer copy/paste currently stays within the open document.

**Mask** turns a vector layer into a clipping source and initially links the next normal layer. Other normal layers can select that mask in Properties. Filled rectangles, ellipses, paths and vector-only symbols clip artwork on the stage and in HTML5. **Show Masking** switches between the clipped result and editable source shapes. Text/bitmap mask sources and masks inside mask-source symbols are explicitly rejected; convert these sources to vector artwork first.

**Guide** layers remain visible/editable in the authoring stage and are omitted from published output. These are visual guides, not motion-path constraints. **Show Others Transparent** uses outline display for surrounding layers without changing their published opacity.

## Library and graphic import

Library shows a fitted first-frame preview above its searchable tree. Search matches symbol names, folders, linkage and class names. Sort by Name, Type or Linkage, with reversible order. New Folder and context-menu Organize controls create, rename and move folders/symbols. Empty folders and paths are document data; view filtering and collapsed state are workspace-only. Resource files remain a separate section.

Drag SVG, PNG or JPEG files onto the stage, or choose **File → Import graphics to Stage**. Multiple files create reusable symbols in the selected library folder and one Undo entry. Repeated identical resources are reused. SVG solid fills/strokes become editable vector paths; text uses installed fonts and becomes outlines. Gradients, embedded images, clipping and effects use a rendered image with the original SVG retained, with an explicit import message. Scripts, animation, embedded HTML and external file references are rejected with an explanation. A stored SVG can also be placed from Resource files.

Try `examples/authoring-workspace.nflash` and drag `examples/authoring-badge.svg` onto its stage. Document format 4 preserves layer roles and library folders; older documents remain readable.
