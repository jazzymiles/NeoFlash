// NF3 surfaces A: Properties, Used colours, Tools palette, Stage toolbar, Concept page.
const S = {};
S.typeHeader = (ic, kind, name) => row({ name: 'Type header', w: 'fill', minH: 'size/tab', pad: [0, 'pad/panel'], gap: 'gap/inline' },
  [icon(ic), txt(kind, 'Body Strong', 'text/primary'), txt(name, 'Body', 'text/tertiary', { grow: true })]);
S.hex = (hex, alpha) => K.field('#', hex.replace('#', ''), { unit: (alpha == null ? 100 : alpha) + ' %' });
S.colourRow = c => row({ name: 'Colour row', w: 'fill', gap: 'gap/control', minH: 'size/control' },
  [K.swatch(c[0]), S.hex(c[0], c[2]), row({ name: 'Usage', w: 52, justify: 'MAX' }, [K.small(c[1])]), K.ibtn('to-stage')]);
S.COLOURS = [['#8B9AB1', '5 × text'], ['#F1F4FA', '2 × text'], ['#FF8E4F', '2 × fill'], ['#233043', '1 × fill'], ['#303C4F', '1 × fill'], ['#44546D', '1 × stroke'], ['#2D3C52', '1 × stroke'], ['#66A0FF', '1 × fill']];
S.usedColours = () => [
  row({ name: 'Section Header / Used colours', w: 'fill', h: 'size/section', pad: [0, 0, 0, 'pad/panel'], gap: 4 },
    [icon('chevdown', 'text/tertiary', 12), txt('Used colours', 'Overline', 'text/secondary', { grow: true }), K.check('Only on stage', true), row({ name: 'More', w: 16, h: 'size/section', justify: 'CENTER' }, [icon('more')])]),
  K.body(S.COLOURS.map(S.colourRow).concat([
    row({ name: 'Replace bar', w: 'fill', gap: 'gap/control', minH: 'size/control' },
      [K.t('Replace'), K.swatch('#8B9AB1'), icon('chevright', 'text/tertiary', 12), K.swatch('#3275B8'), spacer(), K.btn('Replace all')])])),
];
S.propsMovieClip = () => K.panel('Properties', { tabs: ['Properties', 'Transform'] }, [
  S.typeHeader('movieclip', 'Movie clip', 'Instance of “Orbit”'),
  K.body([K.line([K.field('Name', 'mainOrbit'), K.btn('Swap…')]),
    K.line([K.select('Movie Clip'), K.ibtn('symbol', { kind: 'Outlined' }), K.ibtn('copy', { kind: 'Outlined' })])]),
  K.section('Position and size', true, { action: 'link' }),
  K.body([K.pair(K.num('X', '640'), K.num('Y', '180')), K.pair(K.num('W', '216'), K.num('H', '228')),
    K.pair(K.num('Scale X', '100', '%'), K.num('Scale Y', '100', '%')), K.pair(K.num('Rotation', '0', '°'), K.num('Skew', '0', '°'))]),
  K.section('Timeline'),
  K.body([K.pair(K.num('Start frame', '1'), K.select('Loop', { label: 'Play' })), K.small('Scripts that call gotoAndPlay take over after it starts.', null, { grow: true, wrap: true })]),
  K.section('Colour effect'),
  K.body([K.pair(K.select('Alpha', { label: 'Style' }), K.num('α', '100', '%')), K.slider()]),
  K.section('Behaviour / linkage', false), K.section('Accessibility', false),
  K.section('Display'),
  K.body([K.line([K.check('Visible', true), K.check('Cache as bitmap', false)]), K.select('Normal', { label: 'Blend' }), K.select('Original', { label: 'Render' })]),
  K.section('Filters', false),
]);
S.propsDocument = () => K.panel('Properties', { tabs: ['Properties', 'Transform'] }, [
  S.typeHeader('file-code', 'Document', 'Ideas in motion'),
  K.section('Document'),
  K.body([K.pair(K.num('W', '960', 'px'), K.num('H', '600', 'px')), K.pair(K.num('FPS', '24'), row({ name: 'Stage colour', gap: 'gap/control', w: 'fill' }, [K.swatch('#141926'), K.field('#', '141926')]))]),
  K.section('Publish'),
  K.body([K.line([K.badge('HTML5 · Rust / WebAssembly', 'Accent')]), K.select('JavaScript', { label: 'Script' }), K.check('Also compile TypeScript', true), K.line([K.btn('Publish settings…')])]),
].concat(S.usedColours()));
S.TOOLS = ['select', 'subselect', 'transform', 'gradient', 'lasso', 'pen', 'text', 'line', 'rect', 'oval', 'polystar', 'pencil', 'brush', 'ink', 'bucket', 'eyedropper', 'eraser', '3d-rotate', '3d-translate', 'bone', 'bind', 'deco', 'spray', 'puppet', 'razor'];
S.chip = (hex, stroke) => {
  const f = box({ name: stroke ? 'Stroke chip' : 'Fill chip', w: 'size/tool', h: 'size/tool', justify: 'CENTER' }, []);
  const r = figma.createRectangle(); r.resize(16, 16); r.cornerRadius = 3;
  if (stroke) { r.fills = []; r.strokes = [rawPaint(hex)]; r.strokeWeight = 3; r.strokeAlign = 'INSIDE'; } else { r.fills = [rawPaint(hex)]; r.strokes = fillOf('border/default'); r.strokeWeight = 1; r.strokeAlign = 'INSIDE'; }
  return add(f, r);
};
S.toolsPalette = cols => {
  const two = cols === 2;
  const grid = kids => row({ name: 'Tool grid', w: 'fill', wrap: true, gap: 'gap/tool', pad: ['pad/tools', 'pad/tools'], justify: 'CENTER' }, kids);
  const RAIL = ['select', 'transform', 'lasso', 'pen', 'text', 'rect', 'pencil', 'bucket', 'eyedropper', 'eraser'];
  const tools = (two ? S.TOOLS : RAIL).map((t, i) => K.tool(t, i === 0 ? 'Selected' : 'Default', two ? ['transform', 'pen', 'rect', 'pencil', 'bucket', 'lasso'].indexOf(t) >= 0 : ['text', 'eyedropper', 'eraser'].indexOf(t) < 0));
  tools.push(K.ibtn('more', { w: 'size/tool' }));
  return col({ name: 'Panel / Tools — ' + (two ? '2 columns' : '1 column'), w: two ? 'width/tools-2' : 'width/tools-1', fill: 'bg/panel', stroke: 'border/default', radius: 'radius/panel', shadow: 'popover', clip: true }, [
    row({ name: 'Grip', w: 'fill', h: 12, justify: 'CENTER', fill: 'bg/raised', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [icon('grip-h', 'text/tertiary', 12)]),
    grid(tools), divider(),
    grid([K.tool('hand'), K.tool('zoom')]), divider(),
    grid(two ? [S.chip('#44546D', true), S.chip('#3275B8'), K.tool('color-default'), K.tool('color-swap')] : [S.chip('#44546D', true), S.chip('#3275B8')]), divider(),
    grid(two ? [K.tool('snap', 'Selected'), K.tool('smooth'), K.tool('straighten'), K.ibtn('mod-object-drawing', { w: 'size/tool' })] : [K.tool('snap', 'Selected')]),
  ]);
};
S.stageToolbar = () => row({ name: 'Stage toolbar', h: 'size/stage-bar', pad: [0, 3], gap: 2, fill: 'bg/raised', stroke: 'border/default', radius: 'radius/panel', shadow: 'popover' }, [
  K.ibtn('hand'), divider(true), K.ibtn('minus'), row({ name: 'Zoom', w: 44, justify: 'CENTER' }, [txt('130%', 'Body', 'text/primary')]), K.ibtn('plus'), divider(true), K.ibtn('fit'), divider(true),
  K.ibtn('ruler', { state: 'Selected' }), K.ibtn('guides', { state: 'Selected' }), K.ibtn('grid', { state: 'Selected' }), K.ibtn('snap-guides', { state: 'Selected' }), K.ibtn('view-settings')]);
S.scenePill = () => row({ name: 'Scene pill', h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', fill: 'bg/raised', stroke: 'border/default', radius: 'radius/panel' }, [icon('scene'), txt('Scene 1', 'Body', 'text/primary'), icon('chevdown', 'text/tertiary', 12)]);
// ---- Concept page
S.densityTriple = build => row({ name: 'Three paradigms', gap: 40, align: 'MIN' }, ['Normal', 'Compact', 'Classic Flash'].map(d => {
  const c = col({ name: d, gap: 12 }, [txt(d, 'Title', 'text/primary')].concat(build())); nfMode(c, null, d); return c; }));
S.PRINCIPLES = [
  ['Density is a property of every atom', 'Heights, inner paddings, gaps, toggle sizes, radii, label columns and default panel widths come from one “NF3 · Density” collection. Type scale and colours stay the same in Normal, Compact and Classic Flash.'],
  ['Density removes waste, not breathing room', 'A denser paradigm lowers heights and default widths: controls 28 / 24 / 22 pt (were 32), inspector 288 / 264 / 248 pt (was 320). The gap between controls (8 / 6 / 6), field padding (10 / 8 / 8) and panel padding (14 / 12 / 10) have floors; tools, the top bar and tabs stay larger than form controls.'],
  ['Nothing jumps', 'One size per dialog and density; a dock keeps its width across tabs; scroll areas never shrink to content; a control keeps its label and place in every context; a row never overflows its panel.'],
  ['One form grid', 'Inspector rows are two equal columns with the label inside the field. Dialogs use a label column and controls at their natural width; a select is never stretched to the dialog.'],
  ['One row per record', 'A colour, a shortcut, a layer, a publish target: one row of size/control with its actions at the end.'],
  ['Accent means one thing', 'The single primary action, selection, the active tool or tab, a toggle that is on, and keyboard focus. Form options are checkboxes; a switch is only for a state that applies at once.'],
];
S.conceptBoard = theme => {
  const pr = col({ name: 'Principles', gap: 14, w: 420 }, S.PRINCIPLES.map((p, i) => col({ name: 'Principle', gap: 3, w: 'fill' }, [txt((i + 1) + '. ' + p[0], 'Title', 'text/primary'), txt(p[1], 'Dialog Body', 'text/secondary', { grow: true, wrap: true })])));
  const b = K.board('NF3 · Concept — ' + theme, 'Evolution of NF2: the same four palettes, Inter / Roboto Mono and icon masters; new metrics, atoms, form grid and panel composition. Each column below is the same component tree in another density mode.', [
    row({ name: 'Concept row', gap: 56, align: 'MIN' }, [pr,
      col({ name: 'Inspector', gap: 16 }, [txt('Properties — instance', 'Title', 'text/primary'), S.densityTriple(() => [S.propsMovieClip()])]),
      col({ name: 'Document', gap: 16 }, [txt('Properties — document with Used colours (one row per colour)', 'Title', 'text/primary'), S.densityTriple(() => [S.propsDocument()])])]),
    row({ name: 'Concept row 2', gap: 56, align: 'MIN' }, [
      col({ name: 'Tools', gap: 16 }, [txt('Tools palette — no clipped title, no holes, fixed height', 'Title', 'text/primary'), S.densityTriple(() => [row({ gap: 16, align: 'MIN' }, [S.toolsPalette(2), S.toolsPalette(1)])])]),
      col({ name: 'Stage chrome', gap: 16 }, [txt('Stage toolbar and scene pill', 'Title', 'text/primary'), S.densityTriple(() => [S.stageToolbar(), S.scenePill()])])]),
  ]);
  nfMode(b, theme, 'Normal');
  return b;
};
async function nf3Concept() {
  const page = nfPage('v3 · Concept'); await figma.setCurrentPageAsync(page);
  for (const n of page.children.slice()) if (n.name.indexOf('NF3 · Concept') === 0) n.remove();
  let y = 0; const out = [];
  for (const theme of ['Light', 'Dark']) { const b = S.conceptBoard(theme); page.appendChild(b); b.x = 0; b.y = y; y += b.height + 200; out.push({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }); }
  return out;
}
