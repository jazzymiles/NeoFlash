// NF3 panels A (group P2): shared helpers and the inspector-width panels Library, Color, Swatches, Align, Transform,
// Info, History, Scene, Project.
const P2 = {};
P2.body = kids => K.body(kids, { pad: ['gap/section', 'pad/panel'] });
P2.foot = kids => row({ name: 'Panel footer', w: 'fill', pad: ['gap/control', 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [1, 0, 0, 0] }, kids);
P2.list = (kids, o) => col(Object.assign({ name: 'List', w: 'fill', pad: 'pad/menu', gap: 1 }, o || {}), kids);
P2.note = (s, mono) => row({ name: 'Note', w: 'fill', pad: ['gap/control', 'pad/panel'], stroke: 'border/subtle', sides: [1, 0, 0, 0] },
  [txt(s, mono ? 'Mono Small' : 'Small', 'text/tertiary', { grow: true, wrap: true })]);
// Icon group: joined icon buttons in one field-like frame. sel = index or list of indexes shown as Selected.
P2.igroup = (icons, sel) => row({ name: 'Icon group', radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
  icons.map((n, i) => K.ibtn(n, { state: [].concat(sel == null ? [] : sel).indexOf(i) >= 0 ? 'Selected' : 'Default' })));
// Tree rows deeper than the three levels of the Tree Row atom: extra 14 pt steps, as in the atom.
P2.deep = (n, node, sel) => row({ name: 'Indent', w: 'fill', pad: [0, 0, 0, 14 * n], radius: 'radius/check', fill: sel ? 'bg/selected' : null }, [node]);
P2.popover = (name, o, kids) => col(Object.assign({ name: 'Popover / ' + name, w: 'width/inspector', fill: 'bg/raised', stroke: 'border/default', radius: 'radius/panel', shadow: 'popover', clip: true }, o || {}), kids);
P2.states = kids => row({ name: 'States', gap: 40, align: 'MIN' }, kids);
P2.state = (label, node) => col({ name: label, gap: 12 }, [txt(label, 'Title', 'text/primary'), node]);
// ---- artwork helpers (literal sizes: drawings, not chrome)
P2.hsl = (h, s, l) => {
  const f = n => { const k = (n + h / 30) % 12, a = s * Math.min(l, 1 - l), v = Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)))); return (v < 16 ? '0' : '') + v.toString(16); };
  return ('#' + f(0) + f(8) + f(4)).toUpperCase();
};
// Linear gradient paint from '#RRGGBB' or '#RRGGBBAA' stops; v = top to bottom.
P2.gp = (stops, v) => ({ type: 'GRADIENT_LINEAR', gradientTransform: v ? [[0, 1, 0], [-1, 0, 1]] : [[1, 0, 0], [0, 1, 0]],
  gradientStops: stops.map((h, i) => { const c = rawPaint(h).color; return { position: i / (stops.length - 1), color: { r: c.r, g: c.g, b: c.b, a: h.length > 7 ? parseInt(h.slice(7, 9), 16) / 255 : 1 } }; }) });
P2.gfill = (node, paints, solid) => { try { node.fills = paints; } catch (e) { node.fills = [rawPaint(solid)]; } return node; };
P2.orbit = d => {
  const f = figma.createFrame(); f.name = 'Orbit'; f.resize(d, d); f.fills = [];
  const E = (s, x, y, fill, stroke) => { const e = figma.createEllipse(); e.resize(s, s); e.fills = fill ? [rawPaint(fill)] : []; if (stroke) { e.strokes = [rawPaint(stroke)]; e.strokeWeight = 1; } f.appendChild(e); e.x = x; e.y = y; };
  E(d - 8, 4, 4, null, '#44546D'); E(d * 0.3, d * 0.35, d * 0.35, '#66A0FF'); E(d * 0.12, d * 0.44, 0, '#FF8E4F');
  return f;
};
// ---- Library
P2.LIB = [['orbit', 'movieclip', 1], ['planet', 'graphic'], ['cta_btn', 'button-symbol'], ['texture.png', 'bitmap'], ['music.mp3', 'sound-file'], ['Inter Regular', 'font']];
P2.library = () => K.panel('Library', {}, [
  P2.body([K.line([K.select('Ideas in motion.nfl'), K.ibtn('pin', { kind: 'Outlined' })]),
    row({ name: 'Preview', w: 'fill', h: 96, fill: '#141926', radius: 'radius/control', justify: 'CENTER' }, [P2.orbit(72)]),
    K.search('Search library'),
    K.line([K.small('7 items', null, { grow: true }), K.ibtn('chevdown')])]),
  divider(),
  P2.list([K.tree('Library root', { chev: true, icon: 'folderopen' })].concat(P2.LIB.map(i => K.tree(i[0], { level: 1, icon: i[1], state: i[2] ? 'Selected' : 'Default' })))),
  P2.foot([K.ibtn('plus'), K.ibtn('folderplus'), K.ibtn('settings'), spacer(), K.ibtn('trash')]),
]);
// ---- Color
P2.well = (ic, hex, on) => row({ name: 'Colour well', h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/control', fill: on ? 'bg/selected' : 'bg/field', stroke: on ? 'border/focus' : 'border/default' },
  [icon(ic, on ? 'accent/default' : 'text/secondary'), K.swatch(hex)]);
P2.picker = hue => {
  const p = row({ name: 'Colour picker', w: 'fill', h: 120, gap: 6 }, [
    P2.gfill(rect('fill', 'fill', hue, { radius: 4 }), [P2.gp(['#FFFFFF', hue]), P2.gp(['#00000000', '#000000'], true)], hue),
    P2.gfill(rect(12, 'fill', hue, { radius: 6 }), [P2.gp(['#FF0000', '#FFFF00', '#00FF00', '#00FFFF', '#0000FF', '#FF00FF', '#FF0000'], true)], hue),
    P2.gfill(rect(12, 'fill', hue, { radius: 6, stroke: 'border/default' }), [P2.gp([hue, hue + '00'], true)], hue)]);
  abs(p, dot(10, null, { stroke: '#FFFFFF', strokeWeight: 2 }), 150, 14);
  return p;
};
P2.gbar = () => {
  const stop = (hex, on) => rect(8, 10, hex, { radius: 2, stroke: on ? 'accent/default' : 'border/strong' });
  return col({ name: 'Gradient bar', w: 'fill', gap: 2 }, [
    P2.gfill(rect('fill', 14, '#FF9900', { radius: 3, stroke: 'border/default' }), [P2.gp(['#FFFFFF', '#FF9900'])], '#FF9900'),
    row({ name: 'Stops', w: 'fill', justify: 'SPACE_BETWEEN' }, [stop('#FFFFFF'), stop('#FFC266', true), stop('#FF9900')])]);
};
P2.bitmaps = list => row({ name: 'Bitmaps', w: 'fill', gap: 'gap/control' }, list.map(h => {
  const b = box({ name: 'Bitmap', h: 52, fill: h[0], radius: 'radius/control', stroke: h[2] ? 'border/focus' : 'border/default', strokeWeight: h[2] ? 2 : 1 });
  markGrow(b, true, false); return P2.gfill(b, [P2.gp([h[0], h[1]])], h[0]);
}));
P2.rgb = (hex, r, g, b) => [K.line([K.seg(['RGB', 'HSB'], 0), S.hex(hex)]), K.line([K.num('R', r), K.num('G', g), K.num('B', b)])];
P2.COLOR = {
  'Solid colour': () => [P2.picker('#FFCC00')].concat(P2.rgb('#FFCC00', '255', '204', '0')),
  'Radial gradient': () => [K.line([K.t('Overflow', null, null, { grow: true }), P2.igroup(['flow-extend', 'flow-reflect', 'flow-repeat'], 0)]), P2.picker('#FF9900')]
    .concat(P2.rgb('#FF9900', '255', '153', '0'), [P2.gbar(), K.line([K.check('Linear RGB', false)])]),
  'Bitmap fill': () => [P2.bitmaps([['#E0823C', '#6B3D1E', 1], ['#3D4A8C', '#1B2040'], ['#4F9A62', '#234A30']]), P2.bitmaps([['#9A9AA6', '#45454C'], ['#C7465C', '#5E1F2B'], ['#33333D', '#17171C']]),
    K.line([K.btn('Import…', { icon: 'plus' }), K.check('Lock fill', true)]), K.small('texture.png · 512×512 · tiles across selection', null, { grow: true, wrap: true })],
};
P2.color = type => K.panel('Color', { tabs: ['Color', 'Swatches'] }, [
  P2.body([row({ name: 'Wells', w: 'fill', gap: 'gap/control' }, [P2.well('bucket', type === 'Radial gradient' ? '#FF9900' : '#FFCC00', true), P2.well('pencil', '#000000'), spacer(),
    row({ name: 'Colour actions' }, [K.tool('color-default'), K.tool('color-none'), K.tool('color-swap')])]),
    K.select(type, { label: 'Type' })].concat(P2.COLOR[type]())),
  K.section('Stroke'),
  K.body([K.pair(K.num('Width', '1.0', 'px'), K.select('Solid', { label: 'Style' })), K.btn('Add to swatches', { w: 'fill' })]),
]);
P2.colorMenus = () => [
  P2.state('Panel menu (⋯)', K.menu('Color panel', [K.mi('Add swatch', { state: 'Hover' }), K.msep(), K.mi('Help', { key: 'F1' }), K.msep(), K.mi('Close'), K.mi('Close group')])),
  P2.state('Type select, open', K.menu('Color type', ['None', 'Solid colour', 'Linear gradient', 'Radial gradient', 'Bitmap fill'].map((l, i) => K.mi(l, { check: i === 1, state: i === 3 ? 'Hover' : 'Default' }))))];
// ---- Swatches (panel tab and popover share the same sections)
P2.swRow = (hexes, extra) => row({ name: 'Swatch row', gap: 1 }, hexes.map(h => K.swatch(h)).concat(extra || []));
P2.swatchKids = () => {
  const hues = [0, 30, 60, 90, 120, 150, 180, 210, 240, 270, 300, 330];
  const grid = [P2.swRow(hues.map((h, i) => P2.hsl(0, 0, i / 11)))].concat([0.86, 0.74, 0.62, 0.5, 0.4, 0.3, 0.2].map(l => P2.swRow(hues.map(h => P2.hsl(h, 0.85, l)))));
  const recent = P2.swRow(['#00FFCC', '#FF9900', '#5B8CFF', '#F58E4E', '#141926', '#FFFFFF', '#E5484D', '#3DD68C', '#FFCC00', '#8E3DC8', '#1A1A1A', '#7F8AA3']);
  recent.children[0].strokes = fillOf('accent/default'); recent.children[0].strokeWeight = 2;
  const add = box({ name: 'Add swatch', w: 'size/swatch', h: 'size/swatch', justify: 'CENTER' }, [icon('plus', 'text/secondary', 12)]);
  const grads = [['#000000', '#FFFFFF'], ['#FFFFFF', '#FF9900'], ['#5B8CFF', '#141926'], ['#FF0000', '#5E0000'], ['#00FFCC', '#0B4A3E'], ['#FFCC00', '#E5484D']]
    .map(g => P2.gfill(K.swatch(g[1]), [P2.gp(g)], g[1]));
  return [
    P2.body([K.line([K.swatch('#00FFCC'), S.hex('#00FFCC'), row({ name: 'Colour actions' }, [K.ibtn('nocolor'), K.ibtn('eyedropper'), K.ibtn('colorwheel')])])]),
    K.section('Recent'), K.body([recent]),
    K.section('Document'), K.body([P2.swRow(['#141926', '#5B8CFF', '#F58E4E', '#F1F4FA', '#7F8AA3', '#2B5BB5'], [add])]),
    K.section('Palette'), K.body([col({ name: 'Palette', gap: 1 }, grid)]),
    K.section('Gradients'), K.body([row({ name: 'Gradients', gap: 'gap/control' }, grads)]),
  ];
};
P2.swatches = () => K.panel('Swatches', { tabs: ['Color', 'Swatches'], sel: 1 }, P2.swatchKids().concat(S.usedColours()));
// ---- Align, Transform, Info
P2.align = () => K.panel('Align', {}, [
  P2.body([K.line([K.t('Align to', null, null, { grow: true }), K.seg(['Selection', 'Stage'], 0)])]),
  K.section('Align'), K.body([K.line([P2.igroup(['align-left', 'align-hcenter', 'align-right']), P2.igroup(['align-top', 'align-vmiddle', 'align-bottom'])])]),
  K.section('Distribute'), K.body([K.line([P2.igroup(['dist-left', 'dist-hcenter', 'dist-right']), P2.igroup(['dist-top', 'dist-vmiddle', 'dist-bottom'])])]),
  K.section('Spacing'), K.body([K.line([P2.igroup(['space-h', 'space-v']), K.num('Gap', '16', 'px')])]),
  K.section('Match size'), K.body([K.line([P2.igroup(['match-w', 'match-h', 'match-both'])])]),
]);
P2.transform = () => K.panel('Transform', {}, [
  K.section('Position and size', true, { action: 'link' }),
  K.body([K.pair(K.num('X', '64'), K.num('Y', '337')), K.pair(K.num('W', '220'), K.num('H', '48'))]),
  K.section('Scale', true, { action: 'link' }),
  K.body([K.pair(K.num('W', '100', '%'), K.num('H', '100', '%'))]),
  K.section('Rotate and skew'),
  K.body([K.pair(K.num('Rotation', '0', '°'), K.num('Skew', '0', '°'))]),
  P2.foot([P2.igroup(['rotate-ccw', 'rotate-cw', 'flip-h', 'flip-v']), spacer(), K.ibtn('reset-transform')]),
]);
P2.reg = sel => col({ name: 'Registration picker', w: 'size/control', h: 'size/control', gap: 3, justify: 'CENTER', align: 'CENTER', radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
  [0, 1, 2].map(r => row({ name: 'Dots', gap: 3 }, [0, 1, 2].map(c => dot(3, r * 3 + c === sel ? 'accent/default' : 'text/tertiary')))));
P2.info = () => {
  const ro = (l, v) => K.num(l, v, null, { state: 'Disabled' });
  return K.panel('Info', {}, [
    P2.body([K.pair(ro('W', '163.0'), ro('X', '412.0')), K.pair(ro('H', '163.0'), ro('Y', '160.0')), K.line([K.t('Registration', null, null, { grow: true }), P2.reg(4)])]),
    K.section('Cursor'), K.body([txt('X 486   Y 233', 'Mono', 'text/primary')]),
    K.section('Colour'), K.body([K.line([K.swatch('#5B8CFF'), txt('R 91 G 140 B 255 A 100%', 'Mono', 'text/primary', { grow: true })])]),
  ]);
};
// ---- History, Scene, Project
P2.history = () => K.panel('History', {}, [
  P2.list([K.mi('Oval', { check: false }), K.mi('Move', { check: false }), K.mi('Fill colour', { check: true }), K.mi('Transform', { check: false, state: 'Disabled' }), K.mi('Delete', { check: false, state: 'Disabled' })]),
  P2.foot([K.ibtn('undo'), K.ibtn('redo'), spacer(), K.ibtn('trash')]),
]);
P2.scene = () => K.panel('Scene', {}, [
  P2.body([K.search('Find scene')]),
  P2.list(['Intro', 'Scene 1', 'Gallery', 'Outro'].map((s, i) => K.tree(s, { icon: 'scene', state: i === 1 ? 'Selected' : 'Default' }))),
  P2.foot([K.ibtn('plus'), K.ibtn('copy'), spacer(), K.ibtn('trash')]),
]);
P2.project = () => K.panel('Project', {}, [
  P2.body([K.select('Ideas in motion (project)'), K.search('Find file')]),
  P2.list([K.tree('src', { chev: true, icon: 'folderopen' }), K.tree('com/neoflash', { level: 1, chev: true, icon: 'folderopen' }),
    K.tree('Orbit.as', { level: 2, icon: 'file-code', state: 'Selected', meta: '●' }), K.tree('Planet.as', { level: 2, icon: 'file-code' }), K.tree('Main.as', { level: 2, icon: 'file-code' }),
    K.tree('assets', { chev: false }), K.tree('publish', { chev: false }), K.tree('ideas.nfl', { icon: 'film' }), K.tree('project.json', { icon: 'settings' })]),
  P2.foot([K.ibtn('new-file'), K.ibtn('folderplus'), K.ibtn('settings'), K.ibtn('trash'), spacer(), K.btn('Publish')]),
  P2.note('● unsaved changes · Orbit.as is linked to symbol "orbit"'),
]);
async function nf3BuildPanelsA() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Panel / Library', 'Inspector width (was 300 pt). Library select with Pin, preview, search and the item count with its sort control stay in this order; tree rows are size/row; New symbol, New folder, Properties and Delete in the footer.', [P2.library()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Color', 'Three fill types side by side: the header, the Fill / Stroke wells with Default, None and Swap, the Type select and the Stroke section never move. RGB / HSB and #HEX with alpha share one row; R G B are three equal fields. Gradient: Overflow as an icon group, Linear RGB as a checkbox under the gradient bar. Bitmap fill: Lock fill is a checkbox.',
    [P2.states([P2.state('Solid colour', P2.color('Solid colour')), P2.state('Radial gradient', P2.color('Radial gradient')), P2.state('Bitmap fill', P2.color('Bitmap fill'))])], { noteW: 860 }));
  out.push(nfBoard(page, 'Menu / Color panel', 'The two open menus of the Color panel. The type menu is checkable, so every item has the check column.', [P2.states(P2.colorMenus())], { noteW: 460 }));
  out.push(nfBoard(page, 'Panel / Swatches', 'Current colour row (swatch · #HEX with alpha · None · Eyedropper · System picker), then collapsible Recent, Document (+ adds the current colour), Palette and Gradients, then the shared Used colours section (R-106). Replace colour across project and Import palette… live in the ⋯ menu.', [P2.swatches()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Align', 'Align to stays on top as a segmented choice; Align, Distribute, Spacing and Match size are sections with joined icon groups; Gap is a labelled field.', [P2.align()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Transform', 'Two equal columns in every row. The two constrain toggles moved into the section headers (Position and size, Scale); rotate, flip and Reset transform sit in the footer.', [P2.transform()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Info', 'Read-only geometry as two equal columns (W · X, H · Y), the registration picker in its own row, Cursor and Colour readouts as sections so the monospaced values never truncate.', [P2.info()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / History', 'One row per step at size/row; the check column marks the current state, undone steps are disabled. Undo, Redo and Clear history in the footer.', [P2.history()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Scene', 'Search, one row per scene, Add / Duplicate / Delete in the footer.', [P2.scene()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Project', 'Project select and search on top, the file tree at size/row (● = unsaved), New file / New folder / Settings / Delete and Publish in the footer, the link note as the last line.', [P2.project()], { noteW: 420 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
