// NF3 menus A: menu helpers and the main menu dropdowns (File … Help).
const M3 = {};
// Item spec: 'Label|shortcut|flags'. Flags: > submenu, x checked, d disabled, h hover. '-' = separator, '#Caption' = section caption.
// A menu with checkable items is built with { check: true } so that every row gets the check column.
M3.sec = (label, check) => row({ name: 'Menu Section', w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/inline' },
  [check ? box({ name: 'Check column', w: 16, h: 16 }, []) : null, txt(label, 'Overline', 'text/tertiary', { grow: true })]);
M3.item = (s, check) => {
  if (s === '-') return K.msep();
  if (s[0] === '#') return M3.sec(s.slice(1), check);
  const p = s.split('|'), f = p[2] || '';
  return K.mi(p[0], { key: p[1] || null, check: check ? f.indexOf('x') >= 0 : null, sub: f.indexOf('>') >= 0,
    state: f.indexOf('d') >= 0 ? 'Disabled' : f.indexOf('h') >= 0 ? 'Hover' : 'Default' });
};
M3.menu = (name, spec, o) => { o = o || {}; return K.menu(name, spec.map(s => M3.item(s, !!o.check)), o.w ? { w: o.w } : null); };
M3.cap = (name, node) => col({ name: name, gap: 8 }, [txt(name, 'Title', 'text/primary'), node]);
M3.named = (name, spec, o) => M3.cap(name, M3.menu(name, spec, o));
M3.rowOf = kids => row({ name: 'Menus', gap: 32, align: 'MIN' }, kids);

// ---- File, Edit
M3.file = () => M3.named('File', ['New|⌘N', 'Open…|⌘O', 'Open recent||>', '-', 'Close|⌘W', 'Close All|⌥⌘W', '-',
  'Save|⌘S', 'Save as…|⇧⌘S', 'Revert', '-', 'Import||>', 'Export||>', '-', 'Publish settings…|⇧F12', 'Publish|⇧⌥F12', '-', 'Print…|⌘P|d']);
M3.edit = () => M3.named('Edit', ['Undo Fill colour|⌘Z', 'Redo|⇧⌘Z', '-', 'Cut|⌘X', 'Copy|⌘C', 'Paste in centre|⌘V', 'Paste in place|⇧⌘V',
  'Duplicate|⌘D', 'Clear|⌫', '-', 'Select all|⌘A', 'Deselect|⇧⌘A', '-', 'Find and replace|⌘F', 'Timeline||>h', '-',
  'Edit symbols|⌘E', 'Edit selected', 'Edit in place', '-', 'Send to AI…', '-', 'Preferences…|⌘U', 'Keyboard shortcuts…|⌥⇧⌘K'], { w: 240 });
M3.editTimeline = () => M3.named('Edit ▸ Timeline', ['Cut frames|⌥⌘X', 'Copy frames|⌥⌘C', 'Paste frames|⌥⌘V', 'Clear frames|⌥⌫', 'Remove frames|⇧F5', '-',
  'Copy motion', 'Paste motion', 'Paste motion special…', '-', 'Select all frames|⌥⌘A', 'Reverse frames', 'Synchronise symbols||d']);

// ---- View, Insert, Modify
M3.view = () => M3.named('View', ['Zoom in|⌘=', 'Zoom out|⌘−', '100 %|⌘1', 'Fit in window|⌘0', 'Magnification||>', '-',
  'Preview mode||>d', 'Pasteboard|⇧⌘W|xd', '-', 'Rulers|⌥⇧⌘R|x', 'Grid||>h', 'Guides||>', 'Snapping||>', 'Onion Skin||>', '-',
  'Rulers, guides & grid…', 'Show shape hints|⌥⌘H', 'Stage toolbar||>', 'Stage A/B||>'], { check: true, w: 240 });
M3.viewGrid = () => M3.named('View ▸ Grid', ["Show grid|⌘'|x", "Snap to grid|⇧⌘'", 'Show over objects', '-', 'Edit grid…|⌥⌘G'], { check: true });
M3.insert = () => M3.named('Insert', ['New symbol…|⌘F8', '-', 'Layer', 'Layer folder', 'Motion guide', '-', 'Frame|F5', 'Keyframe|F6', 'Blank keyframe|F7', '-',
  'Classic tween', 'Shape tween', 'Motion tween', '-', 'Timeline Marker at Playhead', 'Scene'], { w: 240 });
M3.modify = () => M3.named('Modify', ['Document…|⌘J', '-', 'Convert to symbol…|F8', 'Convert to Bitmap||d', 'Break apart|⌘B', '-',
  'Bitmap||>', 'Symbol||>d', 'Shape||>', 'Combine objects||>', '-', 'Timeline||>', 'Commands & JSFL||>', 'Transform||>', 'Arrange||>', 'Align||>', '-',
  'Group|⌘G', 'Ungroup|⇧⌘G']);

// ---- Text, Control, Debug
M3.text = () => M3.named('Text', ['Size||>', 'Style||>', 'Align||>', 'Letter Spacing||>', '-', 'Font Embedding…']);
M3.control = () => M3.named('Control', ['Play|↵', 'Rewind|⌥⇧R', 'Go to end|⌥⇧.', 'Step forward one frame|.', 'Step back one frame|,', '-',
  'Test movie|⌘↵', 'Test scene|⌥⌘↵', '-', 'Loop playback||x', 'Play all scenes', 'Mute sounds|⌥⌘M', '-',
  'Enable simple buttons|⌥⌘B', 'Live ActionScript', '-', 'Work Area||>'], { check: true, w: 260 });
M3.debug = () => M3.named('Debug', ['Debug Movie', 'Stop Debugging||d', '-', 'Step Over||d', 'Step In||d', 'Step Out||d', '-',
  'Toggle Breakpoint', 'Remove All Breakpoints||d', '-', 'Debugger', 'Compiler errors|⌥F2', 'Output|F2'], { check: true, w: 240 });

// ---- Window, Help (the product has no Document menu: Document… is in Modify, the document list closes Window)
M3.windowMenu = () => M3.named('Window', ['Minimize|⌘M', 'Zoom', 'Duplicate window|⌥⌘K|d', '-',
  'Panels||>h', 'Hide panels|F4', 'Inspector priority|⌥⌘F3', 'Library priority|⌥⌘L', '-',
  'Workspaces||>', 'Appearance||>', 'Density||>', 'Detach panel||>', 'Dock panel||>', 'Customize tools…', 'UI Layout Studio…', '-',
  'Next document|⌃⇥', 'Previous document|⌃⇧⇥', 'Close other documents', '-', 'Ideas in motion||x', 'Character study'], { check: true, w: 260 });
M3.windowPanels = () => M3.named('Window ▸ Panels', ['Brushes', '-', 'Timeline|⌥⌘T|x', 'Tools|⌘F2|x', 'Properties|⌘F3|x', 'Library|⌘L|x',
  'Actions|F9', 'Align|⌘K', 'Color|⇧⌘F9', 'Swatches|⌘F9', 'Transform|⌘T', 'Info|⌘I', 'Ease', 'History|⌘F10', 'Scene|⇧F2', 'Output|F2',
  'Compiler errors|⌥F2', 'Accessibility|⇧F11', '-', 'Motion editor', 'Debugger', 'Project', 'Code snippets', 'Movie Explorer', 'Filters'], { check: true });
M3.windowDensity = () => M3.named('Window ▸ Density', ['Normal||x', 'Compact', 'Classic Flash'], { check: true });
M3.help = () => M3.named('Help', ['Search functions…|F1', 'NeoFlash Help', 'Scripting Reference…', '-', 'Open example', '-',
  'Check for Updates…', 'Send Beta Feedback…', '-', 'About NeoFlash']);

async function nf3BuildMenusA() {
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Menu / File, Edit', 'Rows are the v3 Menu Item at size/row; the items follow the shipped menu model: Close All joins File, Print… is not available yet, Duplicate comes before Clear, Edit ▸ Timeline gains Copy motion, Paste motion and Paste motion special…, and Send to AI… is listed only while an AI connection is ready.',
    [M3.rowOf([M3.file(), M3.edit(), M3.editTimeline()])], { noteW: 720 }));
  out.push(nfBoard(page, 'Menu / View, Insert, Modify', 'View gains 100 %, Fit in window, Onion Skin, Stage toolbar and Stage A/B and one check column for its toggles (Preview mode and Pasteboard are not available yet); Insert gains Timeline Marker at Playhead; Modify gains Document…, Convert to Bitmap (not available yet) and Commands & JSFL.',
    [M3.rowOf([M3.view(), M3.viewGrid(), M3.insert(), M3.modify()])], { noteW: 900 }));
  out.push(nfBoard(page, 'Menu / Text, Control, Debug', 'Text and Debug are new against v2.25 (Text is shown with a text box selected, Debug idle with no breakpoints; its last three items toggle panels); Control gains Live ActionScript and the Work Area submenu.',
    [M3.rowOf([M3.text(), M3.control(), M3.debug()])], { noteW: 760 }));
  out.push(nfBoard(page, 'Menu / Window, Help', 'The v2.25 Document menu is gone as in the product: Document… (⌘J) is in Modify, Close and Close All are in File, and Next document, Previous document, Close other documents and the list of open documents close the Window menu. Window follows the shipped order with Minimize, Zoom, UI Layout Studio… and the Density submenu (Normal, Compact, Classic Flash); Planned panels left Window ▸ Panels; Help gains NeoFlash Help, Scripting Reference…, Check for Updates… and Send Beta Feedback….',
    [M3.rowOf([M3.windowMenu(), M3.windowPanels(), M3.windowDensity(), M3.help()])], { noteW: 900 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
