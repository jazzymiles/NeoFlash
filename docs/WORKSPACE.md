# Workspace

NeoFlash uses a compact classic workspace. **Window → Appearance → Light / Dark** changes both ordinary controls and manually painted timeline/stage overlays. Authored colors, fonts, scripts and document history are independent of the theme.

The [UI design system](DESIGN-SYSTEM.md) defines the target visual and interaction standard for future and migrated UI, with [tokens](design-system/tokens.json) and an [interactive reference](design-system/preview.html). It is separate from the implemented workflows described here.

## Panels

Window controls Timeline, Tools, Library, Properties, Actions, Accessibility, Output, Align, Transform and Color. The three utility panels start hidden. A hidden panel can be shown again from the same menu. **F9** reveals Actions and focuses its editor even while typing in another field. Selecting a frame first makes it the Actions target; the timeline's context menu and **Actions · F9** button also open that frame directly. The header identifies the current symbol and frame. Opening Actions pauses preview playback.

All ten panels support **Window → Detach / Dock** and header dragging between the main window and native floating windows. Drag a tab to move one panel; drag the empty internal header/grip to move its entire tab group. Drop on any of the four highlighted edges, to a tab group, or to a group's split target. Top/bottom areas split horizontally; left/right areas split vertically. Resize areas using their visible boundary grip strips and groups at their dividers; the top/bottom grips change Timeline height. Chosen sizes persist separately from project content. Drag outside the targets to open a native floating window. The default right side shows Library above Properties. Floating windows can contain tab groups and horizontal/vertical splits, with drop targets on all four sides. The macOS title bar moves the whole window; a panel tab or internal header performs docking.

The compact **⋯** header menu contains Edit and docking commands, without a separate toolbar row. **Dock** returns a floating panel to the main window. Closing a floating window hides its panels and retains their layout so Window or F9 can reopen them. Escape cancels a panel drag. There is one document, selection, playhead and script editor across these windows. Actions retains the text cursor when moved between its dock and native window; native text shortcuts and the detached Edit menu address that text field.

**Restore Workspace** returns all panels to the default docked layout and sizes. It preserves the chosen theme, authored document, selection, playhead and unfinished drawing. Layout/theme preferences are saved separately by the application; saving a project does not include machine-specific window positions. On macOS, disconnected-display recovery uses the usable areas of all connected screens and moves inaccessible panel title bars back into reach.

Main-window side groups use stacked rows/columns; arbitrary recursive main-window splits, named workspace presets and multiple open documents remain planned. Native menus, VoiceOver and IME need broader platform acceptance.

## Timeline and compact windows

**Play preview / Enter** advances independent nested MovieClips even when the edited timeline contains one frame. **Pause** holds the displayed pose and resumes from the same nested clocks. **Rewind**, scrubbing, symbol navigation and document edits reset that transient state. Mask-aware picking follows the displayed pose. Preview never runs frame/object scripts; use **Test Movie** for scripted behavior.

Layer names and frame cells share one vertical scroll body, with a fixed ruler. Frames scroll horizontally within that body. Tools and Library scroll vertically when space is limited. Properties use aligned label/value rows, and onion controls wrap. Panel controls share a compact system font and stable heights. The Library preview stays above the scrolling browser. Tween arrows have a fixed six-point arrowhead clipped to their own row, independent of their span length. Icons use vector geometry instead of relying on installed symbol glyphs.

Layer names use compact left-aligned text, with row dividers through the name and property columns. Drag the vertical divider beside the layer columns to change their width; the horizontal resize cursor marks the grip. The width is a workspace preference. Double-click a name or choose **Rename layer** in its context menu. Enter or leaving the field commits a nonempty name as one Undo step; Escape cancels. **Frame zoom**, − / + and **Fit** change frame-cell width with adaptive ruler labels and keep the selected frame in view. They do not change authored frames, FPS or duration.

Scrollbars occupy separate strips outside the frame cells. Held content and blank-key intervals extend visibly to the next key or symbol end; the **End** marker separates actual frames from unused space. A key drag previews its destination and scrolls at the horizontal edge. Occupied destinations are refused. Drag a layer name to reorder layers; an insertion line marks the destination. Folders carry their descendants and masks carry their linked layers. Relationships are preserved; dragging does not change folder membership. Release outside the rows or press Escape to cancel. A completed move is one Undo step. Per-layer lengths and frame-range selection/clipboard remain planned; use a blank key to end a layer's visible content. Active onion skin is identified in the stage header because neighbouring-frame ghosts can appear over an otherwise blank frame.

The minimum main window is 1024 × 720 logical points. Rendering and interaction regressions cover this size and larger windows, both themes, long libraries, many layers and long spans. See [the verification record](VERIFICATION.md) for the separately observed native checks and their limits.

## Creating symbols

**F8 / Convert to Symbol** offers a name, Movie Clip / Graphic / Button type and nine registration points. Conversion preserves the artwork's placement and commits as one Undo step. Multiple objects must form a contiguous stack on one layer so grouping does not reorder unrelated artwork. **Insert → New Symbol / ⌘F8** creates an empty library symbol and opens it. Movie Clips and Graphics start with one frame; Buttons start with four states, **Up / Over / Down / Hit**. The Up artwork holds until a new state key is added. Hit artwork defines input geometry and is not painted in HTML5. Add button actions to its instance on the parent timeline or its linked class; state frames are for artwork. Movie Clips and Buttons can export a runtime identifier and bind an existing class; AS3 bases are MovieClip and SimpleButton respectively. Empty `new SimpleButton()` and assignment of arbitrary state display objects remain unsupported. Graphic playback offers Loop, Play once and Single frame, with a separate First frame per placement. Library Properties changes the symbol type for all instances and refuses incompatible class linkage without deleting code. 9-slice guides and legacy SWF library sharing are not implemented.


The compact FPS and duration fields are linked. Editing either retains every frame; duration sets the nearest integer FPS (1–1000). Numeric property controls accept click-to-type or drag-to-scrub, Enter/blur to apply and Escape to cancel.

## Layer roles and operations

Right-click a layer for visibility/locking, insert/delete/rename, copy/cut/paste/duplicate, merge, symbol conversion and folder operations. A folder groups its descendants; hidden or locked folders affect them. Layer Properties selects a parent folder or mask source. Merge with Next Layer preserves animation, sampling tweened spans into keys when necessary. Layer copy/paste currently stays within the open document.

**Mask** turns a vector layer into a clipping source and initially links the next normal layer. Other normal layers can select that mask in Properties. Filled rectangles, ellipses, paths and vector-only symbols clip artwork on the stage and in HTML5. **Show Masking** switches between the clipped result and editable source shapes. Text/bitmap mask sources and masks inside mask-source symbols are explicitly rejected; convert these sources to vector artwork first.

Click a layer’s colored square to switch between normal artwork and outlines. Right-click the square or layer and choose **Outline color…** to set its RGB color; Apply creates one Undo step and Cancel leaves the document unchanged. Folder outlines include their descendants. Outline display applies to the currently edited symbol and its layer folders; a nested symbol’s saved outline preferences do not override its parent’s artwork. Play preview shows original fills and strokes, as does HTML5.

**Guide** layers remain visible/editable in the authoring stage and are omitted from published output. These are visual guides, not motion-path constraints. **Show Others Transparent** uses outline display for surrounding layers without changing their published opacity.

## Library and graphic import

Library shows a fitted first-frame preview above its searchable tree. Search matches symbol names, folders, linkage and class names. Sort by Name, Type or Linkage, with reversible order. New Folder and context-menu Organize controls create, rename and move folders/symbols. Empty folders and paths are document data; view filtering and collapsed state are workspace-only. Resource files remain a separate section.

Drag SVG, PNG or JPEG files onto the stage, or choose **File → Import graphics to Stage**. Multiple files create reusable symbols in the selected library folder and one Undo entry. Repeated identical resources are reused. SVG solid fills/strokes become editable vector paths; text uses installed fonts and becomes outlines. Gradients, embedded images, clipping and effects use a rendered image with the original SVG retained, with an explicit import message. Scripts, animation, embedded HTML and external file references are rejected with an explanation. Stored SVG/PNG/JPEG resources can also be dragged onto the Stage or placed using **Place on Stage**. Drag a Library symbol onto the Stage or use **Place** to create an instance. The Library’s **Import graphics…** button imports and places files; **Resource files → Add resource files…** only stores resources.

Imported graphics are fitted within the page around the drop point or visible view center. If the selected layer is a folder, guide, mask, locked, hidden or outlined, import creates a visible normal **Imported graphics** layer. Placement selects the new objects and stops playback. Native Finder drags use the current desktop pointer, including when the window system does not send ordinary pointer movement events.

Try `examples/authoring-workspace.nflash` and drag `examples/authoring-badge.svg` onto its stage. Document format 4 preserves layer roles and library folders; older documents remain readable.

## Drawing and color

Tools provide separate fill/stroke colors, alpha percentages and stroke width. New rectangles, ellipses, pen and pencil paths use these settings. Properties edit the selected artwork independently. RGB and alpha are stored separately, so making a paint transparent preserves its RGB color. A continuous property edit is one Undo step.


## Tools and Modify

The two-column Tools panel adds Line (N), Brush (B), Paint Bucket (K), Ink Bottle (S), Eyedropper (I), Eraser (E), Lasso (L), Hand (H), Zoom (Z/M) and Gradient Transform (F). Space temporarily pans. Completed drawing gestures form one Undo step. Bucket fills existing closed vectors; Eraser removes whole objects, fills or strokes; Lasso selects whole placements by contact or enclosure. Brush creates round vector strokes. Gradient Transform adjusts an existing fill's center, scale and rotation. Pressure, geometric eraser cuts, gap closing and radial focal handles remain unfinished.

**Modify** offers grouping, safe static-symbol Break Apart, arranging, transforms and Align. Authoring groups preserve runtime identities and scripts; grouped artwork moves as one unit in alignment and spacing. Groups are currently nonnested and require a contiguous stack on one layer. Break Apart rejects animated, scripted or class-bound content rather than losing its behavior.

**Align** provides six alignments, six position distributions, equal gaps, exact horizontal/vertical gaps (including negative overlap), matching dimensions and To Stage. **Transform** provides compact X/Y/W/H, linked scaling, rotation, skew, flip and reset. **Color** edits selection or drawing defaults with independent fill/stroke RGB and alpha. Changing gradient RGB replaces it with solid paint; alpha-only edits preserve gradient stops.
