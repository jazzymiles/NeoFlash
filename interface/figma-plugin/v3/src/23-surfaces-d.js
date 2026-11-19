// NF3 surfaces D: main editor screen, Timeline, UI Layout Studio.
function abs(parent, child, x, y, cons) { parent.appendChild(child); child.layoutPositioning = 'ABSOLUTE'; child.x = x; child.y = y; if (cons) child.constraints = cons; return child; }
S.menuBar = () => row({ name: 'Top bar', w: 'fill', h: 'size/top-bar', pad: [0, 8], gap: 'gap/control', fill: 'bg/panel', stroke: 'border/subtle', sides: [0, 0, 1, 0] },
  [dot(14, 'accent/default'), txt('NeoFlash', 'Body Strong', 'text/primary'),
    row({ name: 'Menus', pad: [0, 6] }, ['File', 'Edit', 'View', 'Insert', 'Modify', 'Text', 'Control', 'Debug', 'Window', 'Help'].map(m => row({ name: m, pad: [0, 7], h: 'size/control' }, [txt(m, 'Body', 'text/secondary')]))),
    spacer(), K.select('Animator', { w: 'width/field-m' }), K.btn('Test Movie', { icon: 'play' }), K.btn('Publish', { kind: 'Primary', icon: 'upload' })]);
S.docTabs = () => row({ name: 'Document tabs', w: 'fill', h: 'size/tab', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('Ideas in motion', true, 'file-code'), K.tab('Untitled-2', false, 'file-code'), K.ibtn('plus')]);
S.statusBar = () => row({ name: 'Status bar', w: 'fill', h: 'size/status', pad: [0, 10], gap: 'gap/section', fill: 'bg/panel', stroke: 'border/subtle', sides: [1, 0, 0, 0] },
  [K.small('Selection tool', 'text/secondary'), K.small('960 × 600 · 24 fps'), spacer(), K.small('HTML5 · JavaScript'), K.small('Saved')]);
S.layerRow = (name, color, sel) => row({ name: 'Layer row', w: 'fill', h: 'size/layer-row', pad: [0, 6, 0, 8], gap: 4, fill: sel ? 'timeline/row-selected' : null, stroke: 'timeline/line', sides: [0, 0, 1, 0] },
  [icon('layer', 'text/secondary', 12), txt(name, 'Small', 'text/primary', { grow: true }), txt('Normal', 'Small', 'text/tertiary'), dot(4, 'text/tertiary'), dot(4, 'text/tertiary'), rect(8, 8, color, { radius: 2 })]);
S.frameRow = (span, tween) => {
  const sp = row({ name: 'Span', w: span * 12, pad: [0, 3], justify: 'SPACE_BETWEEN', fill: tween || 'timeline/static' }, [dot(5, 'timeline/marker'), rect(4, 7, null, { stroke: 'timeline/marker' })]);
  markGrow(sp, false, true);
  return row({ name: 'Frames', w: 'fill', h: 'size/layer-row', fill: 'timeline/empty', stroke: 'timeline/line', sides: [0, 0, 1, 0], clip: true }, [sp]);
};
S.frameRuler = () => {
  const cells = [row({ name: '1', w: 24, pad: [0, 3] }, [txt('1', 'Mono Small', 'text/secondary')])];
  for (let n = 5; n <= 115; n += 5) cells.push(row({ name: String(n), w: 60, justify: 'CENTER' }, [txt(String(n), 'Mono Small', 'text/tertiary')]));
  return row({ name: 'Frame ruler', w: 'fill', h: 'size/timeline-ruler', fill: 'bg/panel', stroke: 'border/subtle', sides: [0, 0, 1, 0], clip: true, align: 'MAX', pad: [0, 0, 3, 0] }, cells);
};
S.timeline = o => {
  o = o || {};
  const ctl = row({ name: 'Timeline controls', w: 'fill', h: 'size/timeline-controls', pad: [0, 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [
    K.ibtn('layerplus'), K.ibtn('folderplus'), K.ibtn('guideplus'), K.ibtn('trash'), divider(true),
    K.ibtn('first'), K.ibtn('stepback'), K.ibtn('play', { state: 'Selected' }), K.ibtn('stepfwd'), K.ibtn('last'), K.ibtn('loop', { state: 'Selected' }), divider(true),
    txt('1 / 120', 'Mono', 'text/primary'), txt('00:00.00', 'Mono', 'text/tertiary'), K.select('Scene 1', { w: 'width/field-s' }), K.num('FPS', '24', null, { w: 'width/num' }), K.num('Duration', '5.00', 's', { w: 116 }),
    spacer(), K.ibtn('onion'), K.ibtn('onionoutline'), K.ibtn('editmulti'), K.ibtn('markers'), divider(true), K.ibtn('centerframe'), K.slider({ w: 96 }), K.ibtn('more')]);
  const layers = col({ name: 'Layer column', w: 'width/layer-column', stroke: 'border/subtle', sides: [0, 1, 0, 0] }, [
    row({ name: 'Layers header', w: 'fill', h: 'size/timeline-ruler', pad: [0, 6, 0, 8], gap: 4, stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [txt('Layers', 'Overline', 'text/secondary', { grow: true }), txt('Mode', 'Small', 'text/tertiary'), icon('eye', 'text/tertiary', 12), icon('lock', 'text/tertiary', 12), icon('layers', 'text/tertiary', 12)]),
    S.layerRow('Orbit instances', '#FF8E4F'), S.layerRow('Accents', 'layer/blue'), S.layerRow('Typography', 'layer/green', true)]);
  markGrow(layers, false, true);
  const frames = col({ name: 'Frame area', clip: true }, [S.frameRuler(), S.frameRow(120), S.frameRow(120), S.frameRow(120)]);
  markGrow(frames, true, true);
  const body = row({ name: 'Timeline body', w: 'fill', align: 'MIN', clip: true }, [layers, frames]); markGrow(body, true, true);
  const head = row({ name: 'Dock tabs', w: 'fill', h: 'size/tab', pad: [0, 4, 0, 0], stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('Timeline', true, 'film'), K.tab('Output', false), K.tab('Compiler errors', false), spacer(), K.ibtn('more')]);
  return col({ name: 'Panel / Timeline', w: o.w || 'fill', h: 'height/bottom-dock', fill: 'bg/panel', stroke: 'border/subtle', sides: [1, 0, 0, 0], clip: true }, [head, ctl, body]);
};
S.stageArt = () => {
  const f = figma.createFrame(); f.name = 'Stage'; f.resize(720, 450); f.fills = [rawPaint('#141926')]; f.clipsContent = true;
  const T = (s, size, hex, x, y) => { const t = figma.createText(); t.fontName = { family: 'Inter', style: 'Regular' }; t.fontSize = size; t.characters = s; t.fills = [rawPaint(hex)]; f.appendChild(t); t.x = x; t.y = y; };
  const E = (d, x, y, fill, stroke) => { const e = figma.createEllipse(); e.resize(d, d); e.fills = fill ? [rawPaint(fill)] : []; if (stroke) { e.strokes = [rawPaint(stroke)]; e.strokeWeight = 1.5; } f.appendChild(e); e.x = x; e.y = y; };
  const l = figma.createRectangle(); l.resize(34, 3); l.fills = [rawPaint('#FF8E4F')]; f.appendChild(l); l.x = 56; l.y = 44;
  T('Ideas in motion.', 40, '#F1F4FA', 54, 70); T('One symbol. Many possibilities.', 16, '#8B9AB1', 56, 138); T('A familiar timeline. A modern canvas.', 16, '#8B9AB1', 56, 164);
  E(190, 470, 90, null, '#44546D'); E(112, 509, 129, null, '#2D3C52'); E(52, 539, 159, '#66A0FF'); E(18, 556, 82, '#FF8E4F');
  return f;
};
S.shell = () => {
  const stage = box({ name: 'Stage area', fill: 'bg/canvas', clip: true }); markGrow(stage, true, true);
  const insp = S.propsDocument(); markGrow(insp, false, true);
  const dock = col({ name: 'Right dock', w: 'width/inspector', stroke: 'border/subtle', sides: [0, 0, 0, 1], clip: true }, [insp,
    row({ name: 'Collapsed group', w: 'fill', h: 'size/tab', pad: [0, 4, 0, 'pad/panel'], fill: 'bg/panel', stroke: 'border/subtle', sides: [1, 0, 0, 0] }, [txt('Library', 'Body Strong', 'text/primary', { grow: true }), K.ibtn('chevup'), K.ibtn('more')])]);
  markGrow(dock, false, true);
  const mid = row({ name: 'Workspace', w: 'fill', align: 'MIN' }, [stage, dock]); markGrow(mid, true, true);
  const root = col({ name: 'Editor window', w: 'width/window', h: 'height/window', fill: 'bg/app', stroke: 'border/default', clip: true }, [S.menuBar(), S.docTabs(), mid, S.timeline(), S.statusBar()]);
  return { root: root, stage: stage };
};
S.shellChrome = stage => {
  const W = stage.width, H = stage.height, R = 18;
  const top = abs(stage, rect(W, R, 'bg/panel', { name: 'Ruler top', stroke: 'border/subtle' }), 0, 0, { horizontal: 'STRETCH', vertical: 'MIN' });
  top.setBoundVariable('height', V('size/stage-ruler'));
  const left = abs(stage, rect(R, H, 'bg/panel', { name: 'Ruler left', stroke: 'border/subtle' }), 0, 0, { horizontal: 'MIN', vertical: 'STRETCH' });
  left.setBoundVariable('width', V('size/stage-ruler'));
  abs(stage, S.stageArt(), Math.round((W - 720) / 2) + 9, Math.round((H - 450) / 2) + 9, { horizontal: 'CENTER', vertical: 'CENTER' });
  const rail = S.toolsPalette(1); abs(stage, rail, R + 12, R + 6);
  abs(stage, S.scenePill(), R + 12 + (rail.width > 200 ? 40 : rail.width) + 12, R + 12);
  const bar = S.stageToolbar(); abs(stage, bar, W - bar.width - 14, H - bar.height - 14, { horizontal: 'MAX', vertical: 'MAX' });
};
S.studio = () => {
  const bar = row({ name: 'Studio toolbar', w: 'fill', h: 'size/timeline-controls', pad: [0, 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [
    K.seg(['Panels', 'Workspace', 'Timeline'], 0), divider(true),
    K.select('Properties — Movie clip', { label: 'Panel', w: 'width/field-l' }), K.btn('Add', { icon: 'plus' }), K.ibtn('grid', { kind: 'Outlined' }), divider(true),
    K.select('Light', { w: 'width/field-s' }), K.seg(['Normal', 'Compact', 'Classic', 'All'], 3), divider(true),
    K.ibtn('undo'), K.ibtn('redo', { state: 'Disabled' }),
    spacer(), K.select('File', { w: 'width/field-s' }), K.btn('Export code', { kind: 'Primary', icon: 'code' })]);
  const H = [['Properties — Movie clip', 0, true, 'sidebar', '288 × auto'], ['Type header', 1, null, 'text', null], ['Name', 1, null, 'text', '100 % × auto', true], ['Position and size', 1, true, 'folderopen', null], ['X · Y', 2, null, 'text', '50 · 50 %'],
    ['W · H', 2, null, 'text', '50 · 50 %'], ['Scale X · Scale Y', 2, null, 'text', '50 · 50 %'], ['Rotation · Skew', 2, null, 'text', '50 · 50 %'], ['Display', 1, true, 'folderopen', null], ['Visible · Cache as bitmap', 2, null, 'check', 'hug'], ['Blend · Render', 2, null, 'text', '50 · 50 %']];
  const left = col({ name: 'Hierarchy', w: 'width/tree', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 1, 0, 0] }, [
    row({ name: 'Tabs', w: 'fill', h: 'size/tab', pad: [0, 4, 0, 0], stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('Hierarchy', true), K.tab('Add', false), spacer(), K.ibtn('folderplus'), K.ibtn('trash')]),
    col({ name: 'Tree', w: 'fill', pad: 'pad/menu', gap: 1 }, H.map(h => K.tree(h[0], { level: h[1], chev: h[2], icon: h[3], meta: h[4], state: h[5] ? 'Selected' : 'Default' })))]);
  markGrow(left, false, true);
  const canvas = col({ name: 'Canvas', gap: 'gap/section', pad: ['pad/panel', 'pad/dialog'], fill: 'bg/canvas', clip: true }, [
    row({ name: 'Canvas bar', w: 'fill', gap: 'gap/section' }, [K.check('Outlines', true), K.check('Size badges', true), K.check('Solo panel', false), spacer(), K.ibtn('minus', { kind: 'Outlined' }), txt('100%', 'Body', 'text/primary'), K.ibtn('plus', { kind: 'Outlined' })]),
    (() => { const t = S.densityTriple(() => [S.propsMovieClip()]); t.itemSpacing = 20; return t; })()]);
  markGrow(canvas, true, true);
  const right = col({ name: 'Node inspector', w: 'width/inspector', fill: 'bg/panel', stroke: 'border/subtle', sides: [0, 0, 0, 1], clip: true }, [
    row({ name: 'Tabs', w: 'fill', h: 'size/tab', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('Node', true), K.tab('Systems', false), K.tab('Code', false)]),
    S.typeHeader('text', 'Field', 'Name'),
    K.section('Node'), K.body([K.field('Label', 'Name'), K.line([K.check('Visible', true), K.check('Enabled', true)])]),
    K.section('Box sizing'), K.body([K.pair(K.select('Percent', { label: 'W' }), K.num('', '100', '%')), K.pair(K.select('Auto', { label: 'H' }), K.field('', 'Hug content', { state: 'Disabled' })),
      K.seg(['25', '33', '50', '100', 'Auto'], 3, { w: 'fill' }), K.pair(K.num('Min W', '0', 'px'), K.num('Max W', '0', 'px'))]),
    K.section('Spacing from the density system'), K.body([K.pair(K.select('gap/control', { label: 'Gap' }), K.field('', '8 · 6 · 6', { state: 'Disabled' })), K.pair(K.select('pad/panel', { label: 'Pad' }), K.field('', '14 · 12 · 10', { state: 'Disabled' }))]),
    K.section('Typography'), K.body([K.select('Body 11 / 14', { label: 'Style' })])]);
  markGrow(right, false, true);
  const foot = row({ name: 'Studio status', w: 'fill', h: 'size/status', pad: [0, 'pad/dialog'], gap: 'gap/section', fill: 'bg/app', stroke: 'border/subtle', sides: [1, 0, 0, 0] }, [K.small('Properties — Movie clip · 38 nodes'), spacer(), K.small('Saved to target/ui-studio/layout.json')]);
  return K.dialog('UI Layout Studio', { w: 'width/studio', h: 'height/studio' }, [bar, S.dialogBody([left, canvas, right]), foot]);
};
async function nf3BuildD() {
  const page = await nfMaster(), out = [];
  const sh = S.shell();
  out.push(nfBoard(page, 'Screen / Editor — main', '1440 × 900 window. Top bar, document tabs, Stage with floating Tools, scene pill and Stage toolbar, right dock at width/inspector, bottom dock with the Timeline, status bar.', [sh.root], { noteW: 1100 }));
  S.shellChrome(sh.stage);
  out.push(nfBoard(page, 'Panel / Timeline', 'Controls in one row of size/timeline-controls; layer rows and the frame ruler follow the density.', [S.timeline({ w: 1176 })], { noteW: 900 }));
  out.push(nfBoard(page, 'Dialog / UI Layout Studio', 'Owner tool. One toolbar row in groups (board · panel · theme and density · history · wrap · file), fixed column widths, and the selected panel previewed in all three densities at once. Spacing is picked from the density system instead of free pixel values.', [S.studio()], { noteW: 1100 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
