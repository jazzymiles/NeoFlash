// NF3 surfaces P1-B: contextual Properties — Graphic, IK bone, Frame and tween, Appearance, Selection colours, Document script language.
// Uses the P1 builders of 30-props-a.js.

// ---- Graphic instance
P1.framePicker = () => {
  const cells = [];
  for (let i = 0; i < 7; i++) cells.push(row({ name: 'Frame ' + (1 + i * 4), w: 28, h: 28, pad: [0, 3], radius: 3, fill: '#141926', stroke: i === 0 ? 'accent/default' : 'border/default',
    justify: i < 2 ? 'MIN' : i < 5 ? 'CENTER' : 'MAX' }, [dot(8, '#FF8E4F')]));
  return row({ name: 'Frame picker', w: 'fill', gap: 2, clip: true }, cells);
};
P1.propsGraphic = () => P1.panel([
  S.typeHeader('graphic', 'Graphic', 'Instance of “planet” · 24 frames'),
  K.body([K.line([P1.note('Graphics have no instance name — their timeline is synced to the parent.'), K.btn('Swap…')])]),
  K.section('Position and size', false, { action: 'link' }),
  K.section('Colour effect', false),
  K.section('Looping'),
  K.body([K.radio('Loop', true), K.radio('Play once', false), K.radio('Single frame', false), K.radio('Loop reverse', false), K.radio('Play once reverse', false),
    K.pair(P1.st('First', '1'), P1.st('Last', '24')),
    P1.framePicker(), P1.note('Click a thumbnail to set the first frame')]),
]);

// ---- IK bone
P1.propsBone = () => P1.panel([
  S.typeHeader('armature', 'IK bone', 'Armature “arm” · bone 2 of 3'),
  K.body([K.line([K.field('Name', 'arm_lower'), K.ibtn('chevup', { kind: 'Outlined' }), K.ibtn('chevdown', { kind: 'Outlined' })])]),
  K.section('Position'),
  K.body([K.pair(K.num('X', '210.4'), K.num('Y', '188.0')), K.pair(K.num('Length', '96', 'px'), K.num('Angle', '−32', '°'))]),
  K.section('Joint: rotation'),
  K.body([K.line([K.check('Enable', true), K.check('Constrain', true)]), K.pair(P1.st('Min', '−45', '°'), P1.st('Max', '90', '°'))]),
  K.section('Joint: X / Y translation', false),
  K.section('Spring'),
  K.body([K.pair(P1.st('Strength', '20'), P1.st('Damping', '40'))]),
  K.section('Armature'),
  K.body([K.select('Authortime', { label: 'Type' }),
    K.pair(K.select('Solid', { label: 'Style' }), P1.st('Speed', '100', '%')), K.check('Enable spring', true)]),
]);

// ---- Frame with a classic tween, Motion tween
P1.propsFrame = () => P1.panel([
  S.typeHeader('frame', 'Frame', 'Layer “hand” · 1–24'),
  K.section('Frame'),
  K.body([K.field('Name', 'intro'), K.select('Name', { label: 'Label type' })]),
  K.section('Tween', true, { action: 'ease' }),
  K.body([K.line([K.select('Classic', { label: 'Tween' }), K.check('Scale', true)]),
    K.pair(K.num('Ease', '−40', '(in)'), K.select('Flash ease')),
    K.pair(K.select('CW', { label: 'Rotate' }), K.num('', '1', 'times', { stepper: true })),
    K.line([K.check('Orient to path', true), K.check('Sync', true), K.check('Snap', false)])]),
  K.section('Frame actions'),
  K.body([K.line([K.btn('Frame script…')]), K.check('Include frame scripts', true)]),
  K.section('Sound', false),
  K.section('Video', false),
]);
P1.propsMotion = () => P1.panel([
  S.typeHeader('tween-motion', 'Motion tween', 'Layer “hand” · 1–60'),
  K.section('Ease', true, { action: 'ease' }),
  K.body([K.pair(K.num('Ease', '−40', '(in)'), spacer())]),
  K.section('Rotation'),
  K.body([K.pair(P1.st('Rotate', '1', '×'), K.select('CW', { label: 'Direction' })), K.check('Orient to path', true)]),
  K.section('Path'),
  K.body([K.pair(P1.st('X', '412.0'), P1.st('Y', '160.0')), K.pair(P1.st('W', '220.0'), P1.st('H', '120.0')),
    K.line([K.check('Show path', true), K.check('Show frame dots', true)])]),
  K.section('Options'),
  K.body([K.check('Sync graphic symbols', true), K.check('Snap to motion guide', false)]),
]);

// ---- Appearance (paint stack, path modifiers and vector effects)
P1.order = () => [K.ibtn('chevup'), K.ibtn('chevdown'), K.ibtn('trash')];
// A paint is two rows: enable + colour record, then its numbers with move up / move down / delete at the end.
P1.paintItem = (kind, hex, alpha, a, b) => col({ name: 'Item / ' + kind, w: 'fill', gap: 'gap/row' }, [
  row({ name: 'Paint', w: 'fill', gap: 'gap/control', minH: 'size/control' }, [row({ name: 'Enable', w: 'width/label' }, [K.check(kind, true)]), K.swatch(hex), S.hex(hex, alpha)]),
  K.line([K.num(a[0], a[1], a[2]), K.num(b[0], b[1], b[2])].concat(P1.order()))]);
P1.item = (title, rows) => col({ name: 'Item / ' + title, w: 'fill', gap: 'gap/row' },
  [K.line([txt(title, 'Body Strong', 'text/primary', { grow: true })].concat(P1.order()))].concat(rows));
P1.one = f => K.pair(f, spacer());
P1.shapeHeader = () => S.typeHeader('pen', 'Shape', 'Drawing object · layer “Accents”');
P1.propsPaints = () => P1.panel([
  P1.shapeHeader(),
  K.section('Graphic style'),
  K.body([K.select('Brand outline', { label: 'Style' }), K.line([K.btn('Save style…'), K.btn('Unlink', { kind: 'Ghost' }), spacer(), K.ibtn('trash')])]),
  K.section('Appearance'),
  K.body([P1.paintItem('Fill', '#6C93FF', 100, ['dX', '0'], ['dY', '0']), divider(),
    P1.paintItem('Fill', '#131824', 40, ['dX', '4'], ['dY', '4']), divider(),
    P1.paintItem('Stroke', '#F1F4FA', 100, ['W', '2', 'px'], ['Offset', '0']), divider(),
    K.line([K.btn('Add fill'), K.btn('Add stroke')]),
    K.line([K.btn('Expand appearance', { kind: 'Ghost' })])]),
]);
P1.propsModifiers = () => P1.panel([
  P1.shapeHeader(),
  K.section('Path modifiers'),
  K.body([K.select('Add modifier'),
    P1.item('Offset path', [P1.one(K.num('Offset', '4', 'px'))]),
    P1.item('Round corners', [P1.one(K.num('Radius', '8', 'px'))]),
    P1.item('Repeater', [K.pair(K.num('Copies', '5'), K.num('Scale', '90', '%')), K.pair(K.num('dX', '24'), K.num('dY', '0')), P1.one(K.num('Rotation', '15', '°'))]),
    P1.item('Mirror', [K.pair(P1.cell(K.check('Mirror X', true)), P1.cell(K.check('Mirror Y', false))), K.pair(K.num('X', '480'), K.num('Y', '300'))])]),
  K.section('Vector effects'),
  K.body([K.select('Add effect'),
    P1.item('Zig zag', [K.pair(K.num('Size', '6', 'px'), K.num('Ridges', '4')), K.check('Smooth', false)]),
    P1.item('Pucker and bloat', [P1.one(K.num('Amount', '20', '%'))]),
    P1.item('Roughen', [K.pair(K.num('Size', '3', 'px'), K.num('Detail', '10')), P1.one(K.num('Seed', '1'))]),
    P1.item('Twist', [P1.one(K.num('Angle', '30', '°'))])]),
]);

// ---- Multi-selection: Selection colours (the one-row colour record of S.usedColours)
P1.SELECTION = [['#8B9AB1', '5 × text'], ['#F1F4FA', '2 × text'], ['#FF8F4F', '3 × fill'], ['#6C93FF', '3 × mixed']];
P1.propsSelection = () => P1.panel([
  S.typeHeader('select', '12 objects selected', 'Mixed selection'),
  K.body([K.line([K.btn('Transform…'), K.btn('Color…')])]),
  K.section('Selection colours'),
  K.body(P1.SELECTION.map(S.colourRow)),
]);

// ---- Document: Publish section, script-language states
P1.propsScript = other => P1.panel([
  S.typeHeader('file-code', 'Document', 'Ideas in motion'),
  K.section('Document', false),
  K.section('Publish'),
  K.body([K.line([K.badge('HTML5 · Rust / WebAssembly', 'Accent')]), K.select('TypeScript', { label: 'Script' }), K.check('Also compile JavaScript', true),
    other ? K.msg('5 scripts are in other languages and stay in the document: 2 compile with a warning, 3 are skipped when publishing.', 'Warning') : null,
    K.line([K.btn('Publish settings…')])]),
  K.section('Used colours', false),
]);

async function nf3BuildPropsB() {
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Panel / Properties — Graphic, IK bone', 'One-line type headers. Graphic: Looping keeps its five radio options, First / Last and the frame picker. IK bone: joint and spring toggles are checkboxes; previous / next bone stay as icon buttons beside the name.',
    [P1.side([P1.propsGraphic(), P1.propsBone()])], { noteW: 600 }));
  out.push(nfBoard(page, 'Panel / Properties — Frame and tween', 'Left: a frame with a classic tween (Frame, Tween, Frame actions as in the editor). Right: a motion tween span. “Edit…” / “Edit curve…” is the ease icon in the header of the section that holds Ease, so the Ease field keeps one place in both contexts.',
    [P1.side([P1.propsFrame(), P1.propsMotion()])], { noteW: 600 }));
  out.push(nfBoard(page, 'Panel / Properties — Appearance', 'Left: graphic style and the paint stack; right: path modifiers and vector effects. A paint is two rows (enable + colour record, then its numbers with move up / move down / delete); deleting a graphic style is the bin icon button after “Unlink”.',
    [P1.side([P1.propsPaints(), P1.propsModifiers()])], { noteW: 600 }));
  out.push(nfBoard(page, 'Panel / Properties — Selection colours', 'Same one-row colour record as Used colours (swatch · #HEX with alpha · usage · select on stage). A colour used by fills and strokes shows the total as “mixed”; the breakdown is in the tooltip of the usage.',
    [P1.propsSelection()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Properties — Document, script language', 'Only the Publish section changes with the script language. Left: every script is in a compatible language. Right: scripts in other languages add the inline warning between the option and “Publish settings…”.',
    [P1.side([P1.propsScript(false), P1.propsScript(true)])], { noteW: 600 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
