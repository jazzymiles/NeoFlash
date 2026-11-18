// NF3 panels B (group P2): Filters, Accessibility, Brushes, Commands, Code snippets, Movie Explorer (inspector width)
// and the wide panels Output, Compiler errors, Debugger, Actions.
P2.empty = (ic, title, text) => col({ name: 'Empty state', w: 'fill', gap: 'gap/row', pad: ['pad/dialog', 'pad/panel'], align: 'CENTER' },
  [icon(ic, 'text/tertiary'), txt(title, 'Body Strong', 'text/secondary'), K.small(text)]);
P2.code = lines => col({ name: 'Code', w: 'fill', pad: [6, 8], gap: 1, radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle', clip: true }, lines.map(l => txt(l, 'Mono Small', 'text/primary')));
// ---- Filters (content from crates/neoflash-editor/src/panels/filters.rs)
P2.filterRow = (name, value, on, sel) => row({ name: 'Filter row', w: 'fill', h: 'size/row', pad: [0, 'pad/panel'], gap: 'gap/inline', fill: sel ? 'bg/selected' : null },
  [K.check('', on), txt(name, 'Body', 'text/primary', { grow: true }), K.small(value)]);
P2.filterFoot = () => P2.foot([K.ibtn('plus'), K.ibtn('copy'), K.ibtn('chevup'), K.ibtn('chevdown'), spacer(), K.ibtn('more'), K.ibtn('trash')]);
P2.blur = (x, y) => [K.pair(K.num('Blur X', x, 'px'), K.num('Blur Y', y, 'px')), K.line([K.check('Lock Blur X and Blur Y', true)])];
P2.filters = target => {
  if (target === 'none') return K.panel('Filters', {}, [P2.empty('filter', 'No selection', 'Select an object on the Stage')]);
  const display = [K.section('Display'), K.body([K.line([K.check('Visible', true), K.check('Cache as bitmap', false)]), K.select('Normal', { label: 'Blend' }), K.select('Original', { label: 'Render' })])];
  if (target === 'frame') return K.panel('Filters', {}, [S.typeHeader('frame', 'Frame', 'Layer "orbit" · Frame 1')].concat(display, [
    K.section('Filters'),
    K.body([K.line([K.t('Target', null, null, { grow: true }), K.seg(['Keyframe', 'Layer'], 0)])]),
    P2.filterRow('Blur', '4 px · High', true, true),
    K.body(P2.blur('4', '4').concat([K.select('High', { label: 'Quality' })]), { pad: ['gap/section', 'pad/panel'] }),
    P2.filterFoot()]));
  return K.panel('Filters', {}, [S.typeHeader('movieclip', 'Movie clip', 'Instance of "orbit"')].concat(display, [
    K.section('Filters'),
    P2.filterRow('Drop shadow', '5 px · 45°', true, true), P2.filterRow('Glow', 'Off · 8 px', false), P2.filterRow('Adjust color', 'Brightness +10', true),
    K.body(P2.blur('5', '5').concat([
      K.pair(K.num('Angle', '45', '°'), K.num('Distance', '5', 'px')),
      K.pair(K.num('Strength', '100', '%'), K.slider()),
      K.select('High', { label: 'Quality' }),
      K.line([K.swatch('#000000'), K.field('Shadow', '000000', { unit: '60 %' })]),
      K.line([K.check('Knockout', false), K.check('Inner shadow', false), K.check('Hide object', false)], { wrap: true })]), { pad: ['gap/section', 'pad/panel'] }),
    P2.filterFoot()]));
};
P2.filterMenus = () => [
  P2.state('Add filter (+)', K.menu('Add filter', ['Drop shadow', 'Blur', 'Glow', 'Bevel', 'Gradient glow', 'Gradient bevel', 'Adjust color'].map(l => K.mi(l)))),
  P2.state('Presets and options (⋯)', K.menu('Filter options', [K.mi('Reset filter'), K.msep(), K.mi('Enable all'), K.mi('Disable all'), K.mi('Remove all'), K.msep(),
    K.mi('Copy filters'), K.mi('Paste filters'), K.msep(), K.mi('Save preset'), K.mi('Apply preset', { sub: true }), K.mi('Delete preset', { sub: true })]))];
// ---- Accessibility
P2.accessibility = () => K.panel('Accessibility', {}, [
  S.typeHeader('movieclip', 'Movie clip', 'Instance of "orbit"'),
  K.body([K.check('Make object accessible', true), K.check('Make child objects accessible', true), K.check('Auto label', false),
    K.field('Name', 'Orbit animation'),
    col({ name: 'Description', w: 'fill', pad: ['gap/control', 'pad/field'], gap: 2, radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
      [K.small('Description'), txt('An orange planet orbiting a blue core.', 'Body', 'text/primary', { grow: true, wrap: true })]),
    K.pair(K.field('Shortcut', '⌥O'), K.num('Tab', '3', null, { stepper: true }))]),
]);
// ---- Brushes
P2.brush = (label, size, art, sel) => row({ name: 'Brush preset', w: 'fill', h: 'size/row', pad: [0, 'pad/panel'], gap: 'gap/control', fill: sel ? 'bg/selected' : null },
  [row({ name: 'Stroke', w: 60, h: 12, gap: 5, justify: 'CENTER' }, art), txt(label, 'Body', 'text/primary', { grow: true }), txt(size, 'Mono Small', 'text/tertiary')]);
P2.stroke = (t, r, tone) => [rect(56, t, tone || 'text/primary', { radius: r })];
P2.brushes = () => K.panel('Brushes', {}, [
  P2.body([K.line([K.search('Find brush'), K.ibtn('settings')])]),
  K.section('Basic'), P2.brush('Round', '12', P2.stroke(4, 2), true), P2.brush('Flat', '16', P2.stroke(2, 0)), P2.brush('Chisel', '10', P2.stroke(6, 0)),
  K.section('Artistic'), P2.brush('Charcoal', '24', [col({ gap: 1 }, [0, 1, 2].map(() => rect(56, 1, 'text/secondary')))]), P2.brush('Ink pen', '6', P2.stroke(3, 2)), P2.brush('Watercolour', '32', P2.stroke(8, 4, 'text/tertiary')),
  K.section('Pattern'), P2.brush('Dots', '14', [0, 1, 2, 3, 4, 5].map(() => dot(4, 'text/primary'))), P2.brush('Vine', '20', [dot(3, 'text/primary'), rect(14, 1, 'text/primary'), dot(3, 'text/primary'), rect(14, 1, 'text/primary'), dot(3, 'text/primary')]),
  P2.body([K.line([K.t('Size'), K.slider(), K.num('', '12', 'px', { w: 'width/num' })])]),
  P2.foot([K.ibtn('plus'), K.ibtn('folderplus'), spacer(), K.ibtn('trash')]),
]);
// ---- Commands: [label, level, icon, folder, selected]
P2.CMD = [['Animation', 0, 'folder', 1], ['PNG sequence to SWF', 1, 'film'], ['Propagate element names across frames', 1, 'pencil'],
  ['Development', 0, 'folder', 1], ['ActionScript', 1, 'folder', 1], ['Declare stage instances', 2, 'code'], ['Export class', 2, 'file-code'], ['Export SWC', 2, 'package'],
  ['Tools', 1, 'folder', 1], ['Execute script file', 2, 'terminal'], ['Inspect bitmap', 2, 'info'], ['Inspect variable', 2, 'help'],
  ['Extensions', 1, 'folder', 1], ['Change module', 2, 'refresh'], ['Sync module', 2, 'link'],
  ['Library', 0, 'folder', 1], ['Delete empty folders', 1, 'trash'], ['Inspect selected items', 1, 'search'], ['Select items by property', 1, 'select', 0, 1], ['Smooth bitmaps', 1, 'bitmap'],
  ['Spreadsheet-edit properties', 1, 'tree'], ['Wrap bitmaps in classes', 1, 'class'],
  ['Stage', 0, 'folder', 1], ['Apply tints', 1, 'colorwheel'], ['Assign unique names', 1, 'text'], ['Create test squares', 1, 'rect'], ['Make circular', 1, 'refresh'],
  ['Randomize selected elements', 1, 'wand'], ['Set 9-slice guides', 1, 'grid'], ['Tools', 0, 'folder', 1], ['Find', 1, 'search']];
P2.commands = () => K.panel('Commands', {}, [
  P2.body([K.line([K.search('Find command'), K.ibtn('folderopen')]), K.small('23 commands')]),
  divider(),
  P2.list(P2.CMD.map(c => K.tree(c[0], { level: c[1], icon: c[2], chev: c[3] ? true : null, state: c[4] ? 'Selected' : 'Default' }))),
  P2.foot([K.ibtn('new-file'), K.ibtn('folderplus'), K.ibtn('info'), spacer(), K.ibtn('refresh'), K.ibtn('trash')]),
]);
// ---- Code snippets
P2.snippets = () => {
  const item = (s, sel) => K.tree(s, { level: 1, icon: 'snippet', state: sel ? 'Selected' : 'Default' }), dir = (s, open) => K.tree(s, { chev: !!open, icon: open ? 'folderopen' : 'folder' });
  return K.panel('Code snippets', {}, [
    P2.body([K.search('Search snippets')]),
    P2.list([dir('Actions', 1), item('Click to go to frame and stop'), item('Click to go to web page'), item('Drag and drop'), dir('Timeline navigation'),
      dir('Animation', 1), item('Rotate continuously', 1), item('Fade in movie clip'), dir('Load and unload'), dir('Audio and video'), dir('Event handlers')]),
    divider(),
    P2.body([K.small('Rotates the selected movie clip every frame. Change 10 to adjust speed.', 'text/secondary', { grow: true, wrap: true }),
      P2.code(['orbit_mc.addEventListener(', '  Event.ENTER_FRAME, spin);', 'function spin(e:Event):void {', '  orbit_mc.rotation += 10;', '}'])]),
    P2.foot([K.btn('Copy', { kind: 'Ghost' }), spacer(), K.btn('Insert at frame 1', { kind: 'Primary' })]),
  ]);
};
// ---- Movie Explorer
P2.explorer = () => K.panel('Movie Explorer', {}, [
  P2.body([K.line([P2.igroup(['font', 'movieclip', 'code', 'video'], [0, 1, 2]), P2.igroup(['frame', 'settings'])]), K.search('Find in movie')]),
  P2.list([K.tree('Ideas in motion.nfl', { chev: true, icon: 'film' }), K.tree('Scene 1', { level: 1, chev: true, icon: 'scene' }), K.tree('Layer "orbit"', { level: 2, chev: true, icon: 'layer' }),
    P2.deep(3, K.tree('Frame 1', { chev: true, icon: 'frame' })), P2.deep(4, K.tree('orbit_mc  ·  orbit', { icon: 'movieclip', state: 'Selected' }), true), P2.deep(4, K.tree('Actions for Frame 1', { icon: 'code' })),
    K.tree('Layer "labels"', { level: 2, chev: false, icon: 'layer' }), K.tree('Layer "music.mp3"', { level: 2, chev: false, icon: 'waveform' }), K.tree('Symbol definitions', { level: 1, chev: false, icon: 'symbol' })]),
  P2.note('Scene 1 › orbit › Frame 1 › orbit_mc', true),
]);
// ---- wide panels: shared rows
P2.dockBar = kids => row({ name: 'Toolbar', w: 'fill', h: 'size/timeline-controls', pad: [0, 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, kids);
P2.main = kids => { const r = row({ name: 'Main', w: 'fill', align: 'MIN' }, kids); markGrow(r, true, true); return r; };
P2.title = t => row({ name: 'Title', w: 'fill', h: 'size/section', pad: [0, 'pad/panel'] }, [txt(t, 'Overline', 'text/secondary')]);
P2.pane = (title, w, kids) => { const c = col({ name: title, w: w, stroke: 'border/subtle', sides: [0, 1, 0, 0], clip: true }, [P2.title(title)].concat(kids)); markGrow(c, !w, true); return c; };
P2.tr = (cells, kind) => row({ name: 'Table row', w: 'fill', h: 'size/row', pad: [0, 'pad/panel'], gap: 'gap/control', fill: kind === 'head' ? 'bg/app' : kind === 'sel' ? 'bg/selected' : null, stroke: 'border/subtle', sides: [0, 0, 1, 0] },
  cells.map((c, i) => txt(c, kind === 'head' ? 'Small Strong' : 'Body', kind === 'head' || i ? 'text/secondary' : 'text/primary', { grow: true })));
// ---- Output, Compiler errors
P2.LOG = [['info', '12:04:18', 'Test Movie started · Rust/WebAssembly runtime 0.9'], ['info', '12:04:18', 'trace: NeoFlash: the stage is ready'],
  ['warning', '12:04:19', 'Font "Inter Regular" is not embedded — fallback used'], ['error', '12:04:21', 'TypeError #1009: orbit_mc is null (Scene 1, frame 30)']];
P2.logRow = l => {
  const c = l[0] === 'error' ? 'danger/default' : l[0] === 'warning' ? 'warning/default' : 'text/primary';
  return row({ name: 'Log row', w: 'fill', h: 'size/row', pad: [0, 'pad/panel'], gap: 'gap/control' },
    [icon(l[0], l[0] === 'info' ? 'text/tertiary' : c), l[1] ? txt(l[1], 'Mono Small', 'text/tertiary') : null, txt(l[2], 'Mono', c, { grow: true })]);
};
P2.log = lines => { const c = col({ name: 'Log', pad: ['pad/menu', 0], fill: 'bg/field', clip: true }, lines.map(P2.logRow)); markGrow(c, true, true); return c; };
P2.output = () => K.panel('Output', { tabs: ['Output', 'Compiler errors'], w: 720, h: 220 }, [
  P2.dockBar([K.select('All', { label: 'Show', w: 'width/field-m' }), K.search('Filter output'), K.ibtn('copy'), K.ibtn('trash')]),
  P2.log(P2.LOG)]);
P2.problem = (ic, title, where, fix) => row({ name: 'Problem', w: 'fill', minH: 'size/control', pad: [0, 4, 0, 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [0, 0, 1, 0] },
  [icon(ic, ic === 'error' ? 'danger/default' : 'warning/default'), txt(title, 'Body', 'text/primary'), txt(where, 'Small', 'text/tertiary', { grow: true }), fix ? K.ibtn('wand') : null, K.ibtn('external')]);
P2.errors = () => K.panel('Compiler errors', { tabs: ['Output', 'Compiler errors'], sel: 1, w: 720, h: 220 }, [
  P2.dockBar([K.badge('1 error', 'Danger'), K.badge('1 warning', 'Warning')]),
  P2.problem('error', '1120: Access of undefined property orbit_mc.', 'Scene 1 · layer "orbit" · frame 1 · Line 19', true),
  P2.problem('warning', '1090: Migration issue — frameRate is read-only at runtime.', 'Scene 1 · layer "orbit" · frame 1 · Line 4')]);
// ---- Debugger
P2.debugger = () => {
  const frame = (s, cur) => row({ name: 'Stack frame', w: 'fill', h: 'size/row', pad: [0, 'pad/panel'], gap: 'gap/inline', fill: cur ? 'bg/selected' : null },
    [icon(cur ? 'exec-arrow' : 'callstack', cur ? 'warning/default' : 'text/secondary'), txt(s, 'Mono', 'text/primary', { grow: true })]);
  const bp = (s, on) => row({ name: 'Breakpoint', w: 'fill', h: 'size/row', pad: [0, 'pad/panel'], gap: 'gap/inline' }, [icon('breakpoint', on ? 'syntax/breakpoint' : 'text/disabled'), K.check(s, on)]);
  return K.panel('Debugger', { tabs: ['Debugger', 'Output'], w: 960, h: 300 }, [
    P2.dockBar([K.btn('Continue', { kind: 'Primary', icon: 'debug-continue' }), K.ibtn('stop'), divider(true), K.ibtn('step-over'), K.ibtn('step-into'), K.ibtn('step-out'), divider(true),
      K.ibtn('breakpoint'), K.btn('Remove all breakpoints', { kind: 'Ghost' }), spacer(), K.badge('Paused · orbit : Frame 1 · line 18 (breakpoint)', 'Warning')]),
    P2.main([
      P2.pane('Call stack', 'width/tree', [frame('orbit : Frame 1 · line 18', true), frame('Animator.play() · 204'), frame('MotionBase.update() · 88'), frame('[global code]')]),
      P2.pane('Variables', null, [P2.tr(['Name', 'Value'], 'head'), P2.tr(['▸ this', 'MovieClip "orbit"']), P2.tr(['▾ animator', 'Animator']), P2.tr(['    target', 'orbit_mc → null']),
        P2.tr(['    speed', '2.5  (changed)'], 'sel'), P2.tr(['▸ orbitXML', 'XML (14 nodes)']), P2.tr(['    frame', '1'])]),
      P2.pane('Watch', 300, [P2.tr(['Expression', 'Value'], 'head'), P2.tr(['orbit_mc.rotation', 'Error: null reference']), P2.tr(['animator.time', '0.75']), P2.tr(['+ add expression']),
        P2.title('Breakpoints'), bp('orbit : Frame 1 · line 18', true), bp('Orbit.as · line 42', true), bp('Main.as · line 7', false)])])]);
};
// ---- Actions. Code lines are 20 pt (Mono line height) and do not follow the density.
P2.CODE = ['import fl.motion.Animator;', 'var orbitXML:XML = <Motion duration="20">', '    <source>', '        <Source frameRate="24" x="76.5" y="40.5">', '            <dimensions>',
  '                <geom:Rectangle width="160" height="160"/>', '            </dimensions>', '        </Source>', '    </source>', '    <Keyframe index="0" tweenSnap="true">', '        <SimpleEase ease="0"/>',
  '    </Keyframe>', '    <Keyframe index="19" x="364.95"/>', '</Motion>;', '', '// play the orbit once the stage is ready', 'var animator:Animator = new Animator(orbitXML, orbit_mc);', 'animator.play();', 'animator.',
  'trace("NeoFlash: the stage is ready");'];
P2.codeLine = (s, i) => {
  const n = i + 1, mark = { 4: 'warning', 17: 'error', 18: 'breakpoint' }[n], fold = [2, 3, 4, 5, 10].indexOf(n) >= 0;
  return row({ name: 'Line ' + n, w: 'fill', h: 20, gap: 4, fill: n === 19 ? 'syntax/current-line' : null }, [
    row({ name: 'Marker', w: 20, h: 20, justify: 'CENTER' }, mark ? [icon(mark, 'syntax/' + mark)] : []),
    row({ name: 'Number', w: 24, h: 20, justify: 'MAX' }, [txt(String(n), 'Mono Small', n === 19 ? 'text/primary' : 'text/tertiary')]),
    row({ name: 'Fold', w: 12, h: 20 }, fold ? [icon('chevdown', 'text/tertiary', 12)] : []),
    txt(s, 'Mono', s.indexOf('//') === 0 ? 'syntax/comment' : 'text/primary', { grow: true })]);
};
P2.actionsSide = () => {
  const c = col({ name: 'Sidebar', w: 'width/tree', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 1, 0, 0], clip: true }, [
    row({ name: 'Search', w: 'fill', pad: ['gap/section', 'pad/panel'] }, [K.search('Search toolbox')]),
    P2.list(['Top level', 'Language elements', 'fl.controls', 'fl.events'].map(p => K.tree(p, { chev: false, icon: 'package' }))
      .concat([K.tree('fl.motion', { chev: true, icon: 'package' }), K.tree('Animator', { level: 1, icon: 'class' }), K.tree('Motion', { level: 1, icon: 'class' })])),
    divider(), P2.title('Script navigator'),
    P2.list([K.tree('Current selection', { chev: true, icon: 'target' }), K.tree('orbit : Frame 1', { level: 1, icon: 'frame', state: 'Selected' }), K.tree('Scene 1', { chev: true, icon: 'scene' }),
      K.tree('labels : Frame 1', { level: 1, icon: 'frame' }), K.tree('Symbol definitions', { chev: false, icon: 'symbol' })])]);
  markGrow(c, false, true); return c;
};
// st: 'full' (language select, code hint, error peek) | 'project' | 'compatible' | 'other' (language badge of the script).
P2.actions = st => {
  const full = st === 'full', other = st === 'other';
  const lang = full ? K.select('ActionScript 3.0', { w: 'width/field-m' }) : K.badge(other ? 'ActionScript 3.0' : st === 'compatible' ? 'JavaScript' : 'TypeScript', other ? 'Warning' : 'Neutral');
  const notice = other ? K.msg('This script is ActionScript 3.0. The project uses TypeScript, so the script is skipped when publishing.', 'Warning')
    : st === 'compatible' ? K.msg('This script is JavaScript. The project uses TypeScript; the script compiles with a warning.', 'Info') : null;
  const ed = col({ name: 'Editor', fill: 'bg/field', pad: [4, 0], clip: true }, P2.CODE.slice(0, full ? 20 : 8).map(P2.codeLine)); markGrow(ed, true, true);
  const p = K.panel('Actions', { w: 960, h: full ? 640 : 360 }, [
    P2.dockBar([lang, divider(true), K.ibtn('plus'), K.ibtn('search'), K.ibtn('target'), divider(true), K.ibtn('check'), K.ibtn('autoformat'), K.ibtn('codehint'), K.ibtn('debug'), divider(true),
      K.ibtn('collapse'), K.ibtn('expand'), divider(true), K.ibtn('comment-block'), K.ibtn('comment-line'), K.ibtn('uncomment'), divider(true), K.ibtn('sidebar', { state: 'Selected' }),
      spacer(), K.btn('Script assist', { kind: 'Ghost', icon: 'wand' }), K.ibtn('help')]),
    notice ? row({ name: 'Language notice', w: 'fill', pad: ['gap/control', 'pad/panel'], gap: 'gap/control', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [notice, K.btn('Mark as TypeScript', { kind: 'Ghost' })]) : null,
    P2.main([P2.actionsSide(), ed]),
    row({ name: 'Script bar', w: 'fill', h: 'size/tab', pad: [0, 'pad/panel', 0, 0], gap: 'gap/control', stroke: 'border/subtle', sides: [1, 0, 0, 0] },
      [K.tab('orbit : Frame 1', true, 'frame'), K.ibtn('pin', { state: 'Selected' }), K.tab('labels : Frame 1', false, 'frame'), spacer(),
        txt('Line 19, Col 10  ·  20 lines', 'Mono Small', 'text/tertiary'), icon('error', 'danger/default'), K.small('1'), icon('warning', 'warning/default'), K.small('1')])]);
  if (full) {
    abs(ed, K.menu('Code hint', [K.mi('play()', { key: 'void', state: 'Hover' }), K.mi('pause()', { key: 'void' }), K.mi('rewind()', { key: 'void' }), K.mi('stop()', { key: 'void' }), K.mi('time', { key: 'int' })]), 150, 386);
    abs(ed, row({ name: 'Error peek', pad: [4, 8], gap: 'gap/control', radius: 'radius/control', fill: 'bg/raised', stroke: 'danger/default', shadow: 'popover' },
      [icon('error', 'danger/default'), txt('1120: Access of undefined property orbit_mc.', 'Body', 'text/primary'), txt('Quick fix ⌘.', 'Body Strong', 'accent/default')]), 280, 280);
  }
  return p;
};
async function nf3BuildPanelsB() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Panel / Filters', 'Built from the current panel code. One row per filter (enable checkbox · name · summary); the selected filter shows its parameters on the two-column grid; Add, Duplicate, Move up / down, Presets and options (⋯) and Remove in the footer. Frame targets add the Keyframe / Layer choice. The blur lock and the enable toggle became checkboxes; presets are submenus of ⋯.',
    [P2.states([P2.state('Movie clip, Drop shadow selected', P2.filters()), P2.state('Frame target', P2.filters('frame')), P2.state('No selection', P2.filters('none'))].concat(P2.filterMenus()))], { noteW: 1100 }));
  out.push(nfBoard(page, 'Panel / Accessibility', 'Contextual: one-line type header. The three options are checkboxes (were switches); Name and Description are full-width text, Shortcut and Tab index share a row.', [P2.accessibility()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Brushes', 'One row per preset at size/row: stroke sample · name · size. Search with Brush options on top, the Size slider with its field above the footer (New brush, New group, Delete).', [P2.brushes()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Commands', 'Search with Open commands folder, the command count, the tree at size/row (was 32 pt rows), New command / New folder / Info and Reload / Delete in the footer.', [P2.commands()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Code snippets', 'Search, tree, then the description and code of the selected snippet; Copy on the left, the single primary action Insert at frame 1 on the right.', [P2.snippets()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Movie Explorer', 'The two filter groups keep their icons (text · symbols · scripts · media, and frames / layers · customise); tree at size/row, the path of the selection as the last line.', [P2.explorer()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Output', 'Bottom-dock panel, 720 pt wide here. One toolbar row: Show select with its label, filter field, Copy and Clear; one log row per message at size/row.', [P2.output()], { noteW: 720 }));
  out.push(nfBoard(page, 'Panel / Compiler errors', 'One row per problem at size/control: icon · message · location · Quick fix · Go to source (was a two-line card). Counts are badges in the toolbar row.', [P2.errors()], { noteW: 720 }));
  out.push(nfBoard(page, 'Panel / Debugger', 'Toolbar in groups (run · step · breakpoints) with the paused state on the right; three fixed panes: Call stack, Variables, Watch with Breakpoints. Continue is the only primary action and now has a label.', [P2.debugger()], { noteW: 900 }));
  out.push(nfBoard(page, 'Panel / Actions', 'Toolbar in one row of size/timeline-controls, grouped by dividers: language · add / find / target · check / format / hint / debug · folding · comments · sidebar, then Script assist and Help. The first state shows the language select with the code hint and the error peek; the other three show the script-language badge, and the notice row with Mark as … when the script is not in the project language.',
    [P2.states([P2.state('Floating, project language select', P2.actions('full')), P2.state('Script in the project language (toolbar)', P2.actions('project')),
      P2.state('Script in a compatible language (toolbar and notice)', P2.actions('compatible')), P2.state('Script in another language, skipped', P2.actions('other'))])], { noteW: 900 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
