# Clipboard workflows

Select objects with V or Q, Shift-click or a marquee. Copy (⌘C) captures the selected objects at the displayed frame without modifying the document. Cut (⌘X) prepares the same complete data before deleting the selection. One Undo restores a cut.

Paste (⌘V) inserts the objects into the active visible, unlocked layer, preserving their relative positions and painter order. The selection's copied bounds are centered in the visible stage area. Paste in Place (Edit menu, ⇧⌘V) retains original coordinates in the current symbol's coordinate system. A nested symbol uses its own editing origin. These operations preserve object transforms; they do not scale artwork to a differently sized stage.

When pasting into an interpolated frame, a key is materialized from the sampled layer before the new objects are added. Unrelated sampled objects remain at their current pose. The existing sampled-key convention inherits the preceding tween; curve-preserving subdivision is not promised. The complete paste, including new library definitions, is one Undo step. Failed candidate validation leaves the destination unchanged; any valid drawing gesture is finished before the paste command, as for other document commands.

Copying instances includes every reachable symbol definition, all their authored keys, playback modes, frame/object scripts, labels and accessibility metadata. Multiple instances sharing a symbol continue to share it after insertion. Within the same open document, live symbol definitions are reused, so an edit made to the library after Copy is reflected in Paste. A removed definition can be recovered from the captured data. Opening another document, even another copy of the same file, creates a separate clipboard session: imported symbols, layers and placements receive fresh, consistent IDs. Save As keeps the current live document session.

Pasted top-level objects always receive new placement IDs. Names and script text are preserved, not rewritten to guess references. Animation retains authored frame indices and uses the destination document's frame rate. Scripts are data during copy/paste and execute only through the existing preview/publishing path.

Text fields, including Actions, use their text selection. Opening Edit temporarily removes text focus; the editor records the recipient and restores it for the chosen text command. Clipboard text operations pause authoring playback before the field is consumed, preventing a command from editing a later frame. Changing the document, symbol, frame, layer or text target cancels a queued text command.

The system clipboard contains a `NEOFLASH-CLIPBOARD` header followed by versioned JSON. It is portable between NeoFlash app sessions and can include source code and authored content. Installed font files are not added, and paths mentioned in scripts are not resolved. Foreign text is ignored when the stage is the recipient; malformed NeoFlash payloads produce a diagnostic. Copying an unsupported or oversized selection does not replace the existing clipboard or delete the selection.

The initial transfer limits are 8 MiB of encoded text, 1,024 reachable symbols, 10,000 aggregate placement records, 4,096 layers, 16,384 keys, 10,000 scripts and 200,000 path commands. Normal document validation still applies, including graph cycles, depth, transforms and expanded scene limits. Unknown clipboard versions or fields are rejected to avoid silently losing content.

This is an object clipboard. Selected frame ranges, top-level frame Actions, native image/SVG clipboard formats and preserving separate source layers during paste are separate planned workflows. Frame scripts inside captured library symbols are included.

Keyboard Paste in Place has one known native adapter boundary: egui sends a paste payload without key-press modifiers. The editor uses Shift state or the following V release. If Shift is released before V within a single input batch, the shortcut can be interpreted as ordinary Paste. The explicit Edit → Paste in Place command reads the clipboard synchronously and always retains its selected mode. Capturing native modifiers at key press remains tracked work.

Automated verification covers the transfer model, actual shared runtime behavior after paste/reopen, and headless egui menu/text events. Actual macOS clipboard exchange remains pending a live desktop check; see [verification](VERIFICATION.md).
