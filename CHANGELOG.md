# Changelog

## Unreleased — Stage cursors and editing chrome

- The Stage shows NF2 cursors for every tool, and they change with what is under the pointer: move over an object, resize arrows on Free Transform handles, rotate just outside a corner, skew on an edge, a point cursor over path points, pen-close over the first pen anchor, an open/closed hand for the Hand tool and motion-path keys, zoom out with ⌥. Rulers, panels and popovers keep system cursors.
- Selected objects show NF2 selection boxes: symbols with their registration cross and transformation point, text with its sizing handle. Free Transform shows the NF2 transform box with eight square handles; the old rotation knob is replaced by Flash-style rotation just outside a corner, and dragging an edge between handles now skews (one Undo step). Dragging anywhere inside the transform box moves the selection.
- Marquee selection, Subselection path points and pen anchors use the NF2 marquee, anchor and Bézier-handle styles. Both themes use the same cursors; the boxes follow the theme's accent colour.
- Rotated objects show the box of their bounds (not rotated); contrast over artwork and the Light-theme transformation point are open design questions (NF2-REQUESTS R-011).

## Unreleased — Panels and menus

- The ✕ in the header of planned dialog previews (Preferences, Document settings, Publish settings, Export image / video / audio, Import video, Sound properties, PolyStar settings, Layer properties, Edit envelope, Stroke style, Font embedding, Find and replace) closes them again; the header drag area used to swallow the click.
- The Symbols & scenes popover preview closes when you click outside it.
- Long menus show all their rows instead of a 400-point scroll box: Window ▸ Output, Accessibility, Scene, Motion editor and the rows below them are directly clickable, and a panel's tab menu no longer stays clipped after a shorter tab menu in the same group was opened.
- Every Window-menu panel opens and closes from the Window menu and from its tab menu (Hide); every planned preview closes from its own header, window dot or by clicking outside; About, New symbol and the unsaved-changes prompt close with their buttons. One editor test now covers all of these.

## Unreleased — Preferences

- Edit ▸ Preferences… ▸ General ▸ Theme switches between Dark and Light: Save applies and remembers the choice, Cancel or closing the dialog discards it, and Reset to defaults returns to Dark. Following the system theme and the other preferences are not available yet.

## Unreleased — Timeline sound

- Attach library sounds to keyframes: the Timeline controls Sound button lists the document's sounds and "Import sound…" (MP3/WAV), and attaching is one Undo step. Layers that hold only sounds show as Sound layers with waveform spans.
- Properties ▸ Sound (shown for a frame with a sound) edits the sound, Effect (left/right channel, fades), Sync Event / Start / Stop / Stream and Repeat count or Loop, shows file information and plays the sound once.
- Flash sync semantics in HTML5 and editor playback: Event plays on every keyframe entry and overlaps, Start does not overlap itself, Stop stops the sound, and Stream follows the playhead — it starts at the right position after a jump, restarts when the timeline loops and stops at the end of its span or when that timeline stops.
- Editor playback plays timeline sounds through the Mac's default output; Control ▸ Mute sounds (⌥⌘M) silences them. Scrubbing is silent.
- AS3 `SoundMixer.stopAll()` and AS2 `stopAllSounds()` also stop timeline sounds.
- Not yet: Edit envelope, Sound properties and real waveforms in Sound spans (the drawing is decorative).

## Unreleased — Legacy FLA import

- Old binary FLA files from Flash 3, 5, MX, 8, CS3 and CS4 now open: 101 of 106 real local documents import (previously 5), and all 8 public binary FLA samples import as checked projects.
- Files that Flash left with stale container metadata (most CS3/CS4 sites) are read the way Flash reads them, following only live streams; nothing is repaired or rewritten.
- A feature NeoFlash cannot represent yet no longer refuses the whole document. Filters, blend modes, bitmap fills, dynamic/rich text styling, sounds and similar details are approximated or kept as a visible "Unsupported source object" placeholder, and the import report lists each case once with a count and location.
- Imports keep more structure: multiple scenes, motion guides, shape tweens, graphic first-frame offsets, zero-length keys and morph data. Legacy Flash 3–MX Actions-panel commands (button handlers, tellTarget, ifFrameLoaded, gotoAndPlay …) are translated to ActionScript 1; anything untranslated stays as a marked comment.
- Russian and other 8-bit names, text and scripts are decoded (Windows-1251/1252, guessed from content and reported).
- The original FLA is embedded in the project in parts up to 16 MiB when it fits; otherwise the report says to keep the original file.
- `scripts/survey-legacy-fla.py` reports how many documents in a folder import and which approximations they need.

## Unreleased — Timeline

- Shape tweens (green spans): create them from the Timeline controls, the frame menu or Insert ▸ Shape tween. Rectangles, ovals and paths morph between keyframes even when their contours differ; fills, strokes and compatible gradients blend. A keyframe without shapes gets an explanation instead of a jump.
- Motion tweens (blue spans) for one symbol instance or text block: moving, scaling, rotating or recolouring it inside the span adds a ◆ property keyframe that keys only what changed, and every property follows its own track. F6 inside the span keys all properties; removing the tween removes its ◆ keys.
- Motion guides: Add motion guide (Timeline, Insert ▸ Motion guide, layer menu) creates "Guide: <layer>" above the layer; classic tweens on the guided layer follow the guide's first stroke in the editor and in HTML5. Guides are never published.
- Scenes: the Scene select lists the document's scenes and adds new ones (also Insert ▸ Scene). Scenes play one after another in HTML5; `currentScene`, `scenes` and `gotoAndPlay(frame, scene)` see them. Control ▸ Play all scenes continues editor playback into the next scene.
- Loop playback toggle (Timeline and Control menu): off stops on the last frame. Edit multiple frames shows every keyframe in the onion range and moves the selection on all of them.
- Frame commands: Clear keyframe (⇧F6), Convert to keyframes, Convert to blank keyframes and Reverse frames in the frame menu and Modify ▸ Timeline.
- Frame view: Short rows, Snap playhead to keyframes (⇧ suspends it) and Timeline placement on any dock edge. These options are saved with the workspace.
- Ease panel (Window ▸ Ease, the Timeline Ease button, Frame menu ▸ Ease…): edit the curve of the tween under the playhead with presets (Linear, Ease In/Out/In-Out, Back, Custom), draggable handles, numeric X1–Y2 and a preview; Apply is one Undo step. Overshooting curves and Flash acceleration can also be typed in Properties.
- Scene panel (Window ▸ Scene, ⇧F2): find, add, duplicate, delete, rename (double-click) and drag scenes into a new play order.
- Motion editor (Window ▸ Motion editor): property tracks of the motion tween under the playhead with their ◆ keys; add or remove one property's key at the playhead, type values, jump between keys and drag a key's value.
- Properties shows Motion tween settings inside a motion tween: ease, rotation turns and direction, Orient to path and the path box.
- Motion path on the stage: a selected motion-tweened instance shows its path with a dot per frame and ◆ key points while the playhead is in its span; drag a point to move that key (one Undo).
- Copy motion / Paste motion (frame menu): copy the classic or motion tween under the playhead and animate another selected object the same way, relative to its own position, scale and rotation. Paste motion special is not available yet.
- Synchronise symbols (frame menu, Edit ▸ Timeline, Modify ▸ Timeline): graphic instances on the selected keyframes continue the nested animation of the keyframe before them.
- Orient to path: motion tweens (Properties — Motion tween) and guided classic tweens turn their objects with the direction of travel in the editor and HTML5. The classic Tween section has no control for it yet; Copy/Paste motion carries it.
- Frame view ▸ Preview and Preview in context: taller rows with a thumbnail of each keyframe span (the layer alone, or the whole stage). Saved with the workspace.
- Documents that use scenes, shape or motion tweens, motion guides, Orient to path, frame sounds or ruler guides are saved as format 6; older documents open unchanged and keep their version until they use one of these features.

## Unreleased — Progress counting v3

- Count ActionScript 2 and 3 in the same eight Flash Player areas, with Working % and Working-or-partial % per area. AIR, Flex and component libraries are shown once, outside every percentage.
- Count product progress by user-visible functions inside each capability group. A group is closed only when all its functions work.
- Declaration statuses change through test-backed reviews that can promote to Working or demote. History recounts every day's catalogue with the same area rules.

## Unreleased — NF2 design system

- Re-audit AS3 core APIs by execution: a new core-surface workflow runs 223 Stage, display, event, filter, text, collection, Date and RegExp members through the real compiler and WASM player; 33 constants are now working and 190 members partial. The Actions toolbox and code hints include them automatically.
- Stage rulers, guides and grid work as in the NF2 design: rulers in pixels, points, inches, centimetres or millimetres with Auto or fixed label steps and a movable zero point; guides dragged out of the rulers, moved, selected, locked, hidden, deleted by dropping on a ruler or with Delete, cleared in one Undo, saved per scene/symbol and never exported; a stage grid with spacing, colour and Over objects. Moves, transform handles, rectangles, ovals, lines, pen anchors and path points snap to guides and the grid, with a snap ring. View ▸ Rulers / Grid / Guides, the zoom-control toggles, shortcuts and the Rulers, guides & grid popover (Save as default) are enabled.
- Actions gains the NF2 sidebar: a toolbox of the ActionScript 2.0/3.0 APIs NeoFlash runs (packages, classes, members with types; search; double-click inserts, adding the AS3 `import`) and a Script navigator that lists and opens every frame, object and project-file script. Project classes appear in the toolbox and in hints as soon as they are written.
- Code hints after `.` and with ⌃Space or **Show code hint**: members of `this`, typed variables, stage instance names, classes and their statics, including inherited members; ↑↓ choose, Return/Tab insert, Esc closes.
- Enable Auto-format (re-indents only, never moves code), Apply block/line comment and Remove comment. Scripts are checked automatically 0.6 s after typing stops; explicit Check still reports to Output. Debug, folding and Script assist remain unavailable.
- Rebuild the editor chrome on the NF2 design system (`crates/neoflash-ui`, generated from the Figma export in `interface/design-system`): Dark by default, bundled Inter and Roboto Mono, 304 NF2 icons, 32-point controls and 28-point rows.
- Use the NF2 window layout: top bar with menus, project title, Unsaved badge, Test Movie and Publish; tools rail; inspector with Properties over Library; one bottom dock with Timeline · Actions · Output · Accessibility; status bar with shortcut hints; breadcrumb, stage size and zoom control floating over the canvas. Saved layouts from earlier builds are replaced once.

## Unreleased — Shared native design system

- Apply shared Light/Dark tokens, compact panel shells, fields and icon targets across the authoring workspace. Keep authored colors separate from editor chrome.
- Use bounded menus with aligned shortcuts and grouped Window commands. Keep Tools paint controls and Library commands reachable while their lists scroll.
- Organize contextual Properties and supporting panels into aligned sections; expose mixed paint values explicitly and apply each property independently.
- Keep timeline rows and marks at fixed logical sizes, with compact onion options in narrow panels and stronger grid/overlay contrast.

## 2026-10-29 — Frame ranges and contextual panels

- Select rectangular frame ranges across layers; copy, cut, paste, move, clear, insert and remove time with one Undo per operation. Include symbol-level frame scripts explicitly; code collisions reject the whole edit.
- Set independent layer ends with a drag handle or a number. Trimming removes later keys; older documents retain their existing exposure until edited.
- Match timeline stacking to the stage: upper layer rows are in front, and layer drags and frame pastes follow the visible order.
- Separate selection and tool properties, use collapsible inspector sections and paired numeric fields, and give the Library a single scrolling list with fixed search and commands.
- Show Closed / Partial / Planned capability groups and the working scope of partial features on the existing progress page.

## 2026-10-28 — Drawing tools and symbol authoring

- Add ten tools, Modify commands and Align/Transform/Color panels, including exact spacing and group-aware layout.
- Use compact numeric fields and linked timeline FPS/duration; retain the frame count when timing changes.
- Add empty symbol creation, Button state editing and AS2/AS3 input/type linkage, plus Graphic first-frame playback.
- Move floating Edit/Dock controls into the header menu and distinguish tab dragging from whole-group dragging. Preserve skew during transform gestures.

## 2026-10-28 — Layers and configurable authoring workspace

- Keep graphic imports visible on the page, including Finder drops and placement while a hidden, locked or special layer is selected. Add Library-to-Stage dragging and a direct Import graphics control.
- Add per-layer outline switches with editable colors, preserved in the document and excluded from HTML5 appearance.
- Separate fill/stroke RGB and alpha editing, apply drawing defaults consistently and group property gestures into one Undo. Align panel forms and use a compact macOS UI font.

- Add vector mask layers shared by the native stage and HTML5, editor-only guide layers, layer folders and a full layer context menu with grouped editing operations.
- Add compact timeline controls, stronger grid/control contrast, smaller left-aligned layer names, row dividers, inline rename and frame zoom while retaining seconds, numeric onion ranges and opacity. Layer columns resize by dragging; layer reordering preserves folders and mask relationships.
- Give dock areas visible resize grips and align menu shortcuts in a separate right-hand column.
- Replace fixed panel locations with four-sided docks, resizable groups, tabs and native floating panels. Support transfer between windows and tab/split groups inside floating windows. Add a compact symbol dialog and a library with preview, folders, search, sorting and visible linkage.
- Import SVG/PNG/JPEG through the File menu or by dropping files onto the stage as one Undo step. Solid SVG paths stay editable; richer paint is rendered with the original SVG retained.

See the [compatibility matrix](docs/COMPATIBILITY.md) for current support and limitations, and the [declaration audit](docs/progress/ACTIONSCRIPT.md) for ActionScript coverage. Runtime features below have partial support unless explicitly stated otherwise.

## 2026-10-28 — Timeline editing

- Show held content, blank intervals and the timeline end; improve keyframe dragging with a destination preview, edge scrolling and a separate scrollbar.
- Open frame Actions from the selected timeline context, identify the symbol/frame and focus the code editor. Opening Actions pauses preview playback.
- Mark active onion skin in the stage header. Frame-range operations and independent layer lengths remain unfinished.

## 2026-10-28 — Runtime and browser services

- Add AS2 Stage/context menus, Camera/Microphone, LocalConnection, AMF0 HTTP connections and bitmap PrintJob profiles. Hardware capture and visual system print-preview acceptance remain unverified.
- Extend progressive NetStream buffering and completion, SOAP/XML services, transitions, relative depth and TextField.variable binding.
- Add AS2 data collections/binding, styled text and snapshots, bitmap operations, FileReference/FileReferenceList, Locale tables, dynamic Object accessors/watchpoints, Sound metadata/channel matrices and timeline variable loading.
- Add AS2 primitive boxes, project includes, concrete class accessors and with scopes. Keep AS2 `<>` token handling distinct from AS3 XMLList syntax.
- Add rendered filter profiles, event payload classes, DateTimeFormatter and NumberFormatter/NumberParseResult; move shared date arithmetic into Rust.

## Unreleased

### Authoring

- Add pen, pencil and point editing, compound vector paths, fill rules and interpolation between compatible paths.
- Add cross-layer selection, marquee, grouped duplicate/delete/Undo and Free Transform with proportional/center resizing and multi-object rotation.
- Add object Copy/Cut/Paste and Paste in Place across documents, preserving nested symbols, scripts, resources and accessibility metadata.
- Add Light/Dark themes, persistent panel visibility, detachable native panels and workspace recovery across macOS displays. Preserve Actions cursor state and text clipboard routing during panel relocation.
- Improve timeline scrolling, tween indicators, keyframe release handling and drawing/selection lifecycle. Keep native strokes consistent under nested transforms.
- Add project source editing and checks within Actions, linked classes, portable resource files and authored PNG/JPEG image placement.

### Scripting and data

- Add AS3 numeric Class aliases, int/uint bounds and bound numeric method values; ordinary object property/enumerability controls, public source-class reflection, source-class prototype/constructor identity and dense Array insertAt/removeAt. Share Array Class identity across handlers and project code and retain array receivers in captured trait method values. Extend the existing inventory example with visible calculations, metadata and collection processing.
- Add ActionScript functions, closures, structured control flow, exceptions, optional/rest arguments and Function.call/apply. Improve HTML5 diagnostics for arbitrary thrown values.
- Add AS2/AS3 project classes and imports, linked MovieClip construction, AS3 access control, final/dynamic classes, interfaces and inherited accessors. Preserve distinct AS2 class and receiver semantics.
- Extend Array/String, numeric/URI operations, Date, JSON, RegExp, finite geometry, easing/Tween/CustomEase, callback timers and AsBroadcaster. Individual upstream algorithms are adapted into Rust with attribution.
- Add AS3 Vector/Dictionary and enumeration, shared XML parsing, AS2 XML/XMLNode, AS3 XML/XMLList, namespace-aware E4X selectors/literals and bounded targeted edits.
- Add ByteArray numeric/text IO and codecs, AMF0/AMF3 values and local AS2/AS3 SharedObject persistence, including supported shared references and cycles. Correct malformed-AMF errors so generic Error handlers can recover.

### Display, media and HTML5

- Add runtime display objects, dynamic hierarchy changes, linked timelines, events and Stage input. Expose local MovieClip label/playback/loaded-frame metadata, typed FrameLabel values and a single unnamed Scene snapshot. Add MovieClip buttonMode label transitions and enabled while retaining ordinary mouse/key events, plus subtree timeline stopping and validated mixed-child range removal. Allow authored objects to reorder, interleave with runtime objects, detach and reparent while their source placement is live; retain instance state and listeners. Preserve ordinary AS2 fields whose names match AS3 timeline metadata. Correct Stage mouse-click delivery while retaining secondary-pointer filtering.
- Add transformed dragging, hit tests, hitArea, input flags and legacy pointer callbacks. Hidden objects retain their clocks and state; symbol replacement creates fresh instance state.
- Add runtime Graphics gradients/paths, vector masks, scroll rectangles, exact 2D transforms and nested color transforms.
- Add shared BitmapData pixels, filters, channel operations, capture, bulk byte transfers and PNG output. Add AS3 Loader/LoaderInfo and AS2 MovieClipLoader with cancellation and unload behavior.
- Add dynamic TextField/TextFormat layout and scripted HTML5 text entry, selection/focus, input restrictions and AS2 Selection callbacks.
- Add portable resource and HTTP(S) loading, scripted Sound playback and file-video playback through Video/NetStream. Browser gesture, origin and transport rules apply.
- Add ExternalInterface page callbacks, browser navigation and form submission. Add authored accessibility metadata, semantic keyboard controls and preview zoom/pan.
- Reset publication-owned scripts, timers, input and asynchronous resources on Restart.

### Import, coverage and tooling

- Separate AS3 reporting into ordered Flash Player foundations, later media/services, AIR-only APIs, external libraries and unresolved availability. Show purpose and all four audit statuses without changing the complete catalogue denominator or treating existing partial APIs as new work.
- Fix native banner preview: independent MovieClips advance inside one-frame parent timelines, Pause retains nested poses, and source outline preferences stay local to the edited symbol. Mask-aware picking follows the displayed animation.
- Add direct native Flash MX/CS4 FLA import for the supported `Clock.fla` and `300x600.fla` profiles. Preserve nested symbols/layers, classic tweens, masks, text, bitmap media and original ActionScript; embed the exact FLA and source provenance. Missing installed fonts are nonblocking.
- Extend document version 5 with authored affine skew, gradient paths, color transforms and Flash easing. Imported buttons retain separate Up/Over/Down/Hit frames; Hit geometry remains invisible during playback.
- Add a bounded AS1 timeline profile for Clock, retaining its language/source and resolving dynamic names with SWF6 case rules. Unsupported binary records and script features continue to diagnose.
- Expand XFL preservation, hidden-layer handling and AS2 frame-label/callback support. Import one licensed real CS6 FLA as an editable project; general FLA/SWF import remains incomplete.
- Add a pinned, licensed public FLA corpus with source provenance.
- Base ActionScript coverage on the complete declaration catalogue, with separate implemented-subset, partial, missing and needs-review states. Keep the generated dashboard, daily history and repository summary consistent and in English.
- Add grouped browser-export tooling and fix Rust toolchain lookup in HTML5 verification launchers.

## 0.1.0 — development preview, 2026-10-26

- Start the open-source Rust workspace for macOS 15+ with a classic Flash-inspired editor.
- Add versioned documents, library symbols, nested timelines, keyframes, easing and deterministic authored evaluation.
- Add undo/redo, JSON save/open, basic shapes/text and installed-font discovery.
- Add Actions, JavaScript validation, TypeScript transformation and an explicitly bounded AS2/AS3 parser.
- Add shared Rust/WASM playback, standalone HTML5 publication, initial legacy inspection and a documented XFL subset.
- Add build/check/package scripts, CI and product documentation.
