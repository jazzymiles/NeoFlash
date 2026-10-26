# Workspace

NeoFlash uses a compact classic workspace. **Window → Appearance → Light / Dark** changes both ordinary controls and manually painted timeline/stage overlays. Authored colors, fonts, scripts and document history are independent of the theme.

## Panels

Window controls Timeline, Tools, Library, Properties, Actions, Accessibility and Output. A hidden panel can be shown again from the same menu. **F9** reveals Actions even while typing in another field.

**Window → Detach Tools / Timeline / Actions** opens that panel in a separate native window. Move and resize it using the macOS title bar and edges. **Dock** returns it to the main window; the matching Window command also docks it. Closing a detached window hides the panel, retaining its placement so Window or F9 can reopen it. There is one document, selection, playhead and script editor across these windows. Actions retains the text cursor when moved between its dock and native window; native text shortcuts and the detached Edit menu address that text field.

**Restore Workspace** returns all panels to the default docked layout and sizes. It preserves the chosen theme, authored document, selection, playhead and unfinished drawing. Layout/theme preferences are saved separately by the application; saving a project does not include machine-specific window positions. On macOS, disconnected-display recovery uses the usable areas of all connected screens and moves inaccessible panel title bars back into reach.

Only Tools, Timeline and Actions currently detach. Arbitrary split/tab docking, docking by dragging, named workspace presets and multiple open documents remain planned. Other panels can be hidden and shown in their existing docks. Native menus, VoiceOver and IME need broader platform acceptance.

## Timeline and compact windows

Layer names and frame cells share one vertical scroll body, with a fixed ruler. Frames scroll horizontally within that body. Tools and Library scroll vertically when space is limited; document properties and onion controls wrap. Tween arrows have a fixed six-point arrowhead clipped to their own row, independent of their span length. Icons use vector geometry instead of relying on installed symbol glyphs.

The minimum main window is 1024 × 720 logical points. Rendering and interaction regressions cover this size and larger windows, both themes, long libraries, many layers and long spans. See [the verification record](VERIFICATION.md) for the separately observed native checks and their limits.
