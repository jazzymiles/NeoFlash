// NF3 dialogs C (group D1): Trace bitmap, Customize tools panel, PolyStar tool settings, Find and replace,
// Font embedding. Uses D1.body / D1.okCancel / D1.off / D1.disclosure from 32-dialogs-a.js and 32-dialogs-b.js.
// ---- Trace bitmap
D1.traceArt = () => {
  const f = figma.createFrame(); f.name = 'Artwork'; f.resize(224, 224); f.fills = [rawPaint('#141926')]; f.clipsContent = true;
  const half = (x, name) => { const h = figma.createFrame(); h.name = name; h.resize(112, 224); h.fills = []; h.clipsContent = true; f.appendChild(h); h.x = x; h.y = 0; return h; };
  const R = (p, q, hex) => { const r = figma.createRectangle(); r.resize(q[2], q[3]); r.fills = [rawPaint(hex)]; p.appendChild(r); r.x = q[0]; r.y = q[1]; };
  const E = (p, x, y, d, hex) => { const e = figma.createEllipse(); e.resize(d, d); e.fills = [rawPaint(hex)]; p.appendChild(e); e.x = x; e.y = y; };
  const a = half(0, 'Bitmap'), b = half(112, 'Traced');
  [[64, 64, 48, 96], [52, 76, 60, 72], [76, 52, 36, 120]].forEach(q => R(a, q, '#FF9A5C'));
  [[88, 76, 24, 72], [76, 88, 36, 48]].forEach(q => R(a, q, '#FFC95C'));
  R(a, [0, 168, 112, 56], '#273A66');
  E(b, -62, 50, 124, '#FF9A5C'); E(b, -42, 70, 84, '#FFC95C'); E(b, -50, 164, 300, '#273A66');
  R(f, [111, 0, 2, 224], '#FFFFFF');
  return row({ name: 'Split preview', w: 224, h: 224, clip: true, radius: 'radius/control' }, [f]);
};
D1.sliderRow = (label, value, unit, range) => K.formRow(label, [K.slider({ w: 'width/field-m' }), K.num('', value, unit, { w: 'width/num' }), K.small(range)]);
D1.traceBitmap = () => K.dialog('Trace bitmap', { w: 'width/dialog-l' }, [
  row({ name: 'Body', w: 'fill', gap: 'pad/dialog', align: 'MIN', pad: ['pad/panel', 'pad/dialog'] }, [
    col({ name: 'Preview', w: 224, gap: 'gap/row' }, [D1.traceArt(),
      row({ name: 'Labels', w: 'fill' }, [K.small('Bitmap'), spacer(), K.small('Traced')]),
      txt('texture.png · 512 × 512 → 1 284 paths, 30 colours', 'Mono Small', 'text/tertiary', { grow: true, wrap: true })]),
    col({ name: 'Options', w: 'fill', gap: 'gap/row' }, [
      K.formRow('Preset', [K.select('[Default]', { w: 'width/field-l' })]),
      K.formRow('Mode', [K.select('Colour', { w: 'width/field-l' })]),
      K.formRow('Palette', [K.select('Limited', { w: 'width/field-l' })]),
      D1.sliderRow('Colours', '30', null, '2 – 30'),
      D1.disclosure('Advanced options', true),
      D1.sliderRow('Paths', '50', '%', 'Low – High'),
      D1.sliderRow('Corners', '50', '%', 'Less – More'),
      D1.sliderRow('Noise', '15', 'px', '1 – 100'),
      K.formRow('Method', [K.radio('Abutting', false), K.radio('Overlapping', true)]),
      K.formRow('Create', [K.check('Fills', true), K.check('Strokes', false)]),
      K.formRow('Stroke', [K.num('', '10', 'pt', { w: 'width/field-s', state: 'Disabled' })]),
      K.formRow('Options', [K.check('Snap curves to lines', false), K.check('Ignore white', false)])])])],
  [K.check('Live preview', true), spacer(), K.btn('Cancel'), K.btn('Trace', { kind: 'Primary' })]);
// ---- Customize tools panel
D1.TOOL_LIST = [['3d-rotate', '3D rotation tool', 'W'], ['3d-translate', '3D translation tool', 'G'], ['pen-add', 'Add anchor point tool', '='], ['bind', 'Bind tool', 'M'], ['bone', 'Bone tool', 'M'],
  ['brush', 'Brush tool', 'B'], ['pen-convert', 'Convert anchor point tool', 'C'], ['deco', 'Deco tool', 'U'], ['pen-remove', 'Delete anchor point tool', '−'], ['eraser', 'Eraser tool', 'E'],
  ['eyedropper', 'Eyedropper tool', 'I'], ['transform', 'Free transform tool', 'Q'], ['gradient', 'Gradient transform tool', 'F'], ['hand', 'Hand tool', 'H'], ['ink', 'Ink bottle tool', 'S'],
  ['lasso', 'Lasso tool', 'L'], ['line', 'Line tool', 'N'], ['oval', 'Oval tool', 'O'], ['oval-primitive', 'Oval primitive tool', 'O'], ['bucket', 'Paint bucket tool', 'K'], ['pen', 'Pen tool', 'P'],
  ['pencil', 'Pencil tool', 'Y'], ['polystar', 'PolyStar tool', '—'], ['rect', 'Rectangle tool', 'R'], ['rect-primitive', 'Rectangle primitive tool', 'R'], ['select', 'Selection tool', 'V'],
  ['spray', 'Spray brush tool', 'B'], ['subselect', 'Subselection tool', 'A'], ['text', 'Text tool', 'T'], ['zoom', 'Zoom tool', 'Z']];
// mark: undefined = no marker column, true = the default tool of the slot, false = an empty marker (keeps the icons aligned).
D1.toolRow = (t, sel, mark) => row({ name: 'Tool / ' + t[1], w: 'fill', h: 'size/tool', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/check', fill: sel ? 'bg/selected' : null },
  [mark == null ? null : dot(5, mark ? 'accent/default' : null, { name: 'Default marker' }), icon('tool-' + t[0], null, 20), txt(t[1], 'Body', 'text/primary', { grow: true }), txt(t[2], 'Mono Small', 'text/tertiary')]);
D1.toolList = rows => { const c = col({ name: 'List', pad: 'pad/menu', gap: 1, radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle', clip: true }, rows); markGrow(c, true, true); return c; };
D1.customizeTools = () => {
  const cap = s => txt(s, 'Overline', 'text/tertiary');
  const RAIL = [['select', true], ['transform', true], ['lasso', false], ['pen', true], ['text', false], ['line', false], ['rect', true], ['pencil', true], ['bucket', true], ['eyedropper', false], ['eraser', false], ['bone', true], ['deco', true], ['hand', true]];
  const rail = col({ name: 'Rail', gap: 'gap/row' }, [cap('Rail'),
    col({ name: 'Slots', w: 'width/tools-1', gap: 'gap/tool', pad: ['pad/tools', 0], align: 'CENTER', radius: 'radius/panel', fill: 'bg/app', stroke: 'border/subtle' }, RAIL.map((t, i) => K.tool(t[0], i === 1 ? 'Selected' : 'Default', t[1])))]);
  const avail = col({ name: 'Available', gap: 'gap/row' }, [cap('Available tools · 30'), K.search('Find tool'), D1.toolList(D1.TOOL_LIST.map(t => D1.toolRow(t, t[0] === 'gradient')))]);
  markGrow(avail, true, true);
  const actions = col({ name: 'Actions', w: 'width/field-s', gap: 'gap/row', justify: 'CENTER' }, [K.btn('Add ›', { w: 'fill' }), K.btn('‹ Remove', { w: 'fill' })]);
  markGrow(actions, false, true);
  const slot = ['transform', 'gradient', '3d-rotate', '3d-translate'].map(n => D1.TOOL_LIST.find(t => t[0] === n));
  const current = col({ name: 'Current', gap: 'gap/row' }, [cap('Slot 2 · current selection'), D1.toolList(slot.map((t, i) => D1.toolRow(t, false, i === 0))),
    K.small('Tools in one slot form a flyout group. Drag to reorder; the first tool is the default shown on the rail.', null, { grow: true, wrap: true })]);
  markGrow(current, true, true);
  const body = row({ name: 'Body', w: 'fill', gap: 'pad/panel', pad: ['pad/panel', 'pad/dialog'], align: 'MIN' }, [rail, avail, actions, current]);
  markGrow(body, true, true);
  return K.dialog('Customize tools panel', { w: 'width/dialog-l', h: 'height/dialog-xl' }, [body], D1.okCancel('OK', [K.btn('Restore default', { kind: 'Ghost' })]));
};
// ---- PolyStar tool settings
D1.polystar = () => {
  const s = figma.createStar ? figma.createStar() : figma.createPolygon();
  s.name = 'Shape preview'; s.resize(64, 64); s.pointCount = 5; if (s.type === 'STAR') s.innerRadius = 0.5;
  s.fills = fillOf('bg/selected'); s.strokes = fillOf('text/primary'); s.strokeWeight = 1.5;
  return K.dialog('PolyStar tool settings', { w: 'width/dialog-s' }, [D1.body([
    row({ name: 'Preview', w: 'fill', h: 96, justify: 'CENTER', radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle' }, [s]),
    K.formRow('Style', [K.select('Star', { w: 'width/field-m' })]),
    K.formRow('Sides', [K.num('', '5', null, { w: 'width/field-s', stepper: true })]),
    K.formRow('Point size', [K.num('', '0.50', null, { w: 'width/field-s', stepper: true })]),
    K.small('Point size 0–1: smaller = sharper points', null, { grow: true, wrap: true })])], D1.okCancel());
};
// ---- Find and replace
D1.resultRow = (loc, found, kind) => {
  const head = kind === 'head';
  return row({ name: 'Result', w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/section', fill: head ? 'bg/app' : kind === 'sel' ? 'bg/selected' : null, stroke: 'border/subtle', sides: [0, 0, 1, 0] },
    [txt(loc, head ? 'Small Strong' : 'Body', head ? 'text/secondary' : 'text/primary', { grow: true }), txt(found, head ? 'Small Strong' : 'Mono', head ? 'text/secondary' : 'text/primary', { grow: true })]);
};
D1.findReplace = () => K.dialog('Find and replace', { w: 'width/dialog-l' }, [D1.body([
  row({ name: 'Kind', w: 'fill' }, [K.seg(['Text', 'Font', 'Colour', 'Symbol', 'Sound', 'Video', 'Bitmap'], 0)]),
  K.formRow('In', [K.select('Current document', { w: 'width/field-l' }), K.select('All scenes', { label: 'Scene', w: 'width/field-l' })]),
  K.formRow('Find', [K.field('', 'orbit_mc', { state: 'Focus' })]),
  K.formRow('Replace', [K.field('', 'orbitClip')]),
  K.formRow('Match', [K.check('Whole word', true), K.check('Match case', false), K.check('Regular expression', false), K.check('Live edit', false)]),
  K.formRow('Look in', [K.check('Text field contents', true), K.check('Frames / layers / scenes', true), K.check('ActionScript', true)]),
  col({ name: 'Results', w: 'fill', radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle', clip: true }, [
    D1.resultRow('Location', 'Found', 'head'),
    D1.resultRow('Scene 1 › orbit › Frame 1 · line 17', 'new Animator(orbitXML, orbit_mc)', 'sel'),
    D1.resultRow('Scene 1 › orbit › Frame 1 · line 18', 'orbit_mc.rotation += speed'),
    D1.resultRow('Orbit.as · line 42', 'public var orbit_mc:MovieClip'),
    D1.resultRow('Library › orbit · instance name', 'orbit_mc')]),
])], [K.small('4 results in 3 places'), spacer(), K.btn('Find all', { kind: 'Ghost' }), K.btn('Replace'), K.btn('Replace all', { kind: 'Primary' })]);
// ---- Font embedding
D1.fontEmbedding = () => {
  const R = [['Uppercase [A–Z] (27)', true], ['Lowercase [a–z] (27)', true], ['Numerals [0–9] (11)', true], ['Punctuation (52)', true], ['Basic Latin (95)', false], ['Cyrillic (256)', true], ['Latin Extended A (128)', false], ['Greek (117)', false]];
  const checks = list => col({ name: 'Ranges', w: 'fill', gap: 'gap/row' }, list.map(r => K.check(r[0], r[1])));
  return K.dialog('Font embedding', { w: 'width/dialog-l', h: 'height/dialog-l' }, [S.dialogBody([
    S.side(['Inter Regular', 'Inter Semi Bold', 'Roboto Mono'].map((f, i) => S.navRow('font', f, i === 0)).concat([
      row({ name: 'Font actions', w: 'fill', pad: ['gap/row', 0, 0, 0] }, [K.ibtn('plus'), K.ibtn('minus')])]), true),
    S.page([
      K.formRow('Name', [K.field('', 'Inter Regular')]),
      K.formRow('Family', [K.select('Inter', { w: 'width/field-l' })]),
      K.formRow('Style', [K.select('Regular', { w: 'width/field-l' })]),
      S.group('Character ranges'),
      row({ name: 'Range columns', w: 'fill', gap: 'gap/section', align: 'MIN' }, [checks(R.slice(0, 4)), checks(R.slice(4))]),
      K.formRow('Also include', [K.field('', '→ ← ★ © ®')]),
      S.group('ActionScript'),
      K.formRow('', [K.check('Export for ActionScript', true, D1.off)]),
      K.formRow('Class', [K.field('', 'InterRegular', { state: 'Disabled', w: 'width/field-l' })])])])],
    [K.badge('378 glyphs · subset estimate ~ 46 kB', 'Accent'), spacer(), K.btn('Cancel'), K.btn('OK', { kind: 'Primary' })]);
};
async function nf3BuildDialogsC() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Dialog / Trace bitmap', 'Two columns: the split preview with its result line, and the options on the label grid. Each slider row is slider · value · range, so the range captions no longer take a line of their own; Live preview stays in the footer.',
    [D1.traceBitmap()], { noteW: 720 }));
  out.push(nfBoard(page, 'Dialog / Customize tools panel', 'Fixed size (width/dialog-l × height/dialog-xl): rail · available tools (search, scrolling list of all 30) · Add / Remove · the tools of the selected slot. A dot marks the default tool of the slot; Restore default stays on the left of the footer.',
    [D1.customizeTools()], { noteW: 720 }));
  out.push(nfBoard(page, 'Dialog / PolyStar tool settings', 'width/dialog-s: shape preview on top, Style, Sides and Point size on the label grid with the hint below.',
    [D1.polystar()], { noteW: 420 }));
  out.push(nfBoard(page, 'Dialog / Find and replace', 'Kind switch, scope selects, Find and Replace on the label grid; the two option rows get the labels Match and Look in; one size/row per result; the result count sits in the footer beside Find all, Replace and Replace all.',
    [D1.findReplace()], { noteW: 720 }));
  out.push(nfBoard(page, 'Dialog / Font embedding', 'Same shell as Preferences: the embedded fonts with Add / Remove in the sidebar, one page on the label grid, the character ranges in two columns; the glyph estimate sits in the footer. ActionScript class export is not available yet and stays disabled.',
    [D1.fontEmbedding()], { noteW: 720 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
