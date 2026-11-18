// NF3 menus B: context menus (frame, layer, blank Layers area, Stage object) and the tool flyouts.
M3.frameMenu = () => M3.named('Frame (right-click)', ['Create motion tween', 'Create shape tween', 'Create classic tween||h', '-',
  'Insert frame|F5', 'Remove frames|⇧F5', 'Insert keyframe|F6', 'Insert blank keyframe|F7', 'Clear keyframe|⇧F6',
  'Convert to keyframes', 'Convert to blank keyframes', 'Distribute to keyframes', 'Utils||>', '-',
  'Cut frames|⌥⌘X', 'Copy frames|⌥⌘C', 'Paste frames|⌥⌘V', 'Clear frames|⌥⌫', 'Select all frames|⌥⌘A', '-',
  'Copy motion', 'Paste motion', 'Paste motion special…', '-', 'Reverse frames', 'Synchronise symbols||d', '-',
  'Actions|F9', 'Ease…', 'Copy ease', 'Apply ease'], { w: 240 });
// Shown for a mask layer: Mask is checked, so "Mask mode" and "Invert mask" are listed.
M3.layerMenu = () => M3.named('Layer (right-click)', ['Show all', 'Lock others', 'Hide others', 'Solo', 'Clear all solo||d', '-',
  'Insert layer', 'Delete layer', 'Duplicate layers', 'Cut layers', 'Copy layers', 'Paste layers', '-',
  'Guide', 'Add classic motion guide', 'Mask||x', 'Mask mode: Geometry||>', 'Invert mask', 'Show masking', 'Sync layer to timeline', 'Blending: Normal||>', '-',
  'Insert folder', 'Delete folder||d', 'Expand folder||d', 'Collapse all folders', '-', 'Properties…||h', '-',
  '#More', 'Rename layer', 'Unlock all', 'Outline color…', 'Others transparent', 'Show all as solid', 'Expand all folders',
  'Merge with next layer', 'Convert layers to symbol…', 'Move to folder||>'], { check: true, w: 260 });
M3.blankLayersMenu = () => M3.named('Layers, blank area (right-click)', ['Add new layer', 'Add new folder', 'Add motion guide', '-', 'Paste layers', '-',
  'Show all', 'Clear all solo||d', 'Unlock all', 'Show all as solid', '-', 'Expand all folders', 'Collapse all folders']);
// Shown for a movie clip instance ("Orbit").
M3.stageMenu = () => M3.named('Stage object (right-click)', ['Cut|⌘X', 'Copy|⌘C', 'Paste in place|⇧⌘V', '-',
  'Convert to symbol…|F8', 'Break apart|⌘B', 'Edit in place', 'Edit in new window||d', 'Swap symbol…', 'Swap symbol across span', 'Duplicate symbol…', 'Actions…|F9', '-',
  'Trace bitmap…||d', 'Bitmap properties…||d', '-', 'Create motion tween', 'Distribute to layers|⇧⌘D', 'Distribute to keyframes', '-',
  'Arrange||>', 'Align||>', 'Transform||>', 'Pixel snap (round to whole pixels)', 'Auto Instance Namer…', 'Randomize Selection…', '-',
  'Group|⌘G', 'Ungroup|⇧⌘G', '-', 'Show in library'], { w: 280 });

// ---- Tool flyouts: 16 pt tool icon · "<name> tool" · shortcut key, as the Tools rail draws them. The first tool of a slot is the one shown on the rail.
M3.icon16 = ic => { const i = icon(ic); if (i.width > 16) i.resize(16, 16); return i; };
M3.flyRow = (ic, label, key, selected) => row({ name: 'Flyout item', w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/check', fill: selected ? 'bg/selected' : null },
  [box({ name: 'Icon slot', w: 16, h: 16, justify: 'CENTER' }, [M3.icon16(ic)]), txt(label, 'Body', 'text/primary', { grow: true }), key ? txt(key, 'Small', 'text/tertiary') : null]);
// Default slots that hold more than one tool (tool_preferences.rs); names and keys from the tool table in panels/tools.rs.
M3.FLYOUTS = [
  ['Selection', [['select', 'Selection tool', 'V'], ['subselect', 'Subselection tool', 'A']]],
  ['Transform', [['transform', 'Free Transform tool', 'Q'], ['gradient-transform', 'Gradient Transform tool', 'F']]],
  ['Pen', [['pen', 'Pen tool', 'P'], ['line', 'Line tool', 'N']]],
  ['Shapes', [['rect', 'Rectangle tool', 'R'], ['oval', 'Oval tool', 'O'], ['tool-polystar', 'PolyStar tool', '']]],
  ['Drawing', [['pencil', 'Pencil tool', 'Y'], ['brush', 'Brush tool', 'B']]],
  ['Fill', [['bucket', 'Paint Bucket tool', 'K'], ['tool-ink', 'Ink Bottle tool', 'S']]],
  ['View', [['hand', 'Hand tool', 'H'], ['zoomin', 'Zoom tool', 'Z']]],
];
// Tools outside the default rail: they get a flyout only when the user puts them into one slot in Customize tools.
M3.FLYOUTS_CUSTOM = [
  ['3D', [['tool-3d-rotate', '3D Rotation tool', 'W'], ['tool-3d-translate', '3D Translation tool', 'G']]],
  ['Deco', [['tool-deco', 'Deco tool', 'U'], ['tool-spray', 'Spray Brush tool', '⇧B']]],
  ['Rigging', [['tool-bone', 'Bone tool', 'X'], ['tool-bind', 'Bind tool', '⇧X']]],
];
M3.flyout = g => M3.cap(g[0], K.menu('Flyout / ' + g[0], g[1].map((t, i) => M3.flyRow(t[0], t[1], t[2], i === 0)), { w: 260 }));
M3.flyGroup = (caption, rows) => col({ name: caption, gap: 16 }, [txt(caption, 'Overline', 'text/tertiary')].concat(rows));

async function nf3BuildMenusB() {
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Menu / Context — frame, layer, Stage object', 'Rows at size/row, items in the shipped order. Frame gains Distribute to keyframes, Utils, Copy ease and Apply ease. Layer (shown for a mask layer) gains Solo, mask mode, Invert mask, Sync layer to timeline, blending and the More section; a folder layer adds Collapse folder and a normal layer a Mask submenu. Right-click on the blank Layers area opens the 10-item menu. Stage object (shown for a movie clip instance) gains Break apart, Swap symbol across span, Actions…, the bitmap commands, Distribute to keyframes, the selection utilities and Group / Ungroup; a graphic instance adds Edit in place at current frame and Graphic: Loop / Play once / Single frame (current), a multiple selection adds Reverse stacking order.',
    [M3.rowOf([M3.frameMenu(), M3.layerMenu(), M3.blankLayersMenu(), M3.stageMenu()])], { noteW: 1000 }));
  out.push(nfBoard(page, 'Menu / Tool flyouts', 'Long-press a tool with a corner mark (or click the mark) to open its group. The groups are the shipped default slots that hold more than one tool, with the shipped tool names and keys; each flyout is a menu of size/row rows (16 pt icon, name, key) and the tool shown on the rail carries the selected fill instead of the v2.25 ■ marker. The 3D, Deco, Spray Brush, Bone and Bind tools are not on the default rail: the second group shows them as slots a user can build in Customize tools. The v2.25 anchor point, primitive and Symbol spray rows are gone because the product has no such tools.',
    [col({ name: 'Flyouts', gap: 40 }, [
      M3.flyGroup('Default slots', [M3.rowOf(M3.FLYOUTS.slice(0, 4).map(M3.flyout)), M3.rowOf(M3.FLYOUTS.slice(4).map(M3.flyout))]),
      M3.flyGroup('Example slots built in Customize tools', [M3.rowOf(M3.FLYOUTS_CUSTOM.map(M3.flyout))])])], { noteW: 900 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
