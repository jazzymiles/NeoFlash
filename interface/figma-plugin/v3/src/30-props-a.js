// NF3 surfaces P1-A: contextual Properties — shared builders, Text, Frame sound, Video, Primitives, Drawing tools.
const P1 = {};
P1.panel = kids => K.panel('Properties', { tabs: ['Properties', 'Transform'] }, kids);
P1.side = kids => row({ name: 'States', gap: 40, align: 'MIN' }, kids);
P1.cell = k => row({ name: 'Cell', w: 'fill', minH: 'size/control' }, [k]);
P1.note = s => K.small(s, null, { grow: true, wrap: true });
P1.st = (label, value, unit, o) => K.num(label, value, unit, Object.assign({ stepper: true }, o || {}));
// Position and size: the same rows and order as S.propsMovieClip; the link action constrains proportions.
P1.pos = (x, y, w, h, more) => [K.section('Position and size', true, { action: 'link' }),
  K.body([K.pair(K.num('X', x), K.num('Y', y)), K.pair(K.num('W', w), K.num('H', h))].concat(more || []))];
// One row per paint: swatch + labelled hex field, alpha as the unit (null = no alpha part).
P1.paint = (label, hex, alpha) => row({ name: 'Paint / ' + label, w: 'fill', gap: 'gap/control', minH: 'size/control' },
  [K.swatch(hex), K.field(label, hex.replace('#', ''), { unit: alpha == null ? null : alpha + ' %' })]);
P1.igroup = (icons, sel) => row({ name: 'Icon group', gap: 'gap/tool' }, icons.map((ic, i) => K.ibtn(ic, { state: i === sel ? 'Selected' : 'Default' })));
P1.labelled = (label, node) => row({ name: label, w: 'fill', gap: 'gap/inline', minH: 'size/control' }, [K.small(label, 'text/secondary'), node]);
P1.capJoin = () => K.pair(P1.labelled('Cap', P1.igroup(['cap-none', 'cap-round', 'cap-square'], 1)), P1.labelled('Join', P1.igroup(['join-miter', 'join-round', 'join-bevel'], 1)));
// Labelled value on its own row, then its slider with the range ends around it (the Slider atom needs about 100 pt).
P1.slide = (label, value, unit, lo, hi) => col({ name: 'Slider / ' + label, w: 'fill', gap: 'gap/row' }, [K.pair(K.num(label, value, unit), spacer()),
  row({ name: 'Range', w: 'fill', gap: 'gap/inline', minH: 'size/control' }, [row({ name: 'Min', w: 8 }, [K.small(lo)]), K.slider(), row({ name: 'Max', w: 22, justify: 'MAX' }, [K.small(hi)])])]);
// Record list: one row per record at size/row; bw = width of the value column (text content, does not scale).
P1.list = rows => col({ name: 'List', w: 'fill', stroke: 'border/subtle', radius: 'radius/control', clip: true }, rows);
P1.rec = (a, b, kind, bw) => row({ name: 'Record', w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/control', fill: kind === 'sel' ? 'bg/selected' : kind === 'head' ? 'bg/app' : null },
  [txt(a, kind === 'head' ? 'Small Strong' : 'Body', kind === 'head' ? 'text/secondary' : 'text/primary', { grow: true }),
    row({ name: 'Value', w: bw }, [txt(b, kind === 'head' ? 'Small Strong' : 'Small', 'text/secondary')])]);
P1.strokeRows = (width, hinting) => [K.pair(K.num('Width', width, 'px', { stepper: true }), K.select('Solid', { label: 'Style' })),
  K.pair(K.select('Normal', { label: 'Scale' }), P1.cell(K.check('Stroke hinting', hinting))), P1.capJoin()];

// ---- Text (text box and simple text)
P1.character = tlf => [K.select('Inter'), K.line([K.select('Regular'), K.btn('Embed…')]),
  K.pair(K.num('Size', '24', 'pt'), K.num('Leading', '120', '%')),
  P1.paint('Color', '#1A1A1A', tlf ? 100 : null),
  K.pair(K.num('Tracking', '-5.0'), K.field('Highlight', 'None')),
  K.select('Anti-alias for readability'),
  K.line([P1.igroup(['text-underline', 'text-strikethrough', 'superscript', 'subscript'], -1), spacer(), K.check('Auto kern', true)]),
].concat(tlf ? [K.select('Auto', { label: 'Glyph rotation' })] : []);
P1.propsText = tlf => P1.panel([
  S.typeHeader('text', 'Text', tlf ? 'Text box · TLF' : 'Simple text'),
  K.body(tlf ? [K.pair(K.select('TLF Text (CS5)'), K.select('Editable')), K.pair(K.select('Horizontal'), K.select('LTR'))] : [K.check('Use text box', false)]),
].concat(P1.pos('141.50', '137.85', '303.10', '151.20', [K.pair(K.num('Scale X', '100', '%'), K.num('Scale Y', '100', '%')), K.pair(K.num('Rotation', '0', '°'), K.num('Skew', '0', '°'))]), [
  K.section('Character'), K.body(P1.character(tlf)),
], tlf ? [
  K.section('Advanced character'),
  K.body([K.pair(K.select('Default', { label: 'Case' }), K.select('Auto', { label: 'Break' })), K.select('Default', { label: 'Digit case' }),
    K.select('Proportional', { label: 'Digit width' }), K.select('Common', { label: 'Ligatures' }),
    K.pair(K.num('Baseline', '0', 'px'), K.select('en-US', { label: 'Locale' }))]),
  K.section('Paragraph'),
  K.body([K.line([P1.igroup(['talign-left', 'talign-center', 'talign-right', 'talign-justify', 'talign-justify-all'], 0), K.num('Indent', '0', 'px')]),
    K.pair(K.num('Left', '0', 'px'), K.num('Right', '0', 'px')),
    K.pair(K.num('Before', '0', 'pt'), K.num('After', '0', 'pt')),
    K.select('Inter-word', { label: 'Text justify' })]),
  K.section('Container and flow'),
  K.body([K.select('Multiline', { label: 'Behavior' }), K.select('Top', { label: 'Vertical' }),
    K.pair(K.num('Columns', '2'), K.num('Gutter', '20', 'px')),
    K.pair(K.num('Padding L', '2', 'px'), K.num('Padding R', '2', 'px')),
    K.pair(K.num('Padding T', '2', 'px'), K.num('Padding B', '2', 'px')),
    K.pair(K.num('Border', '1', 'pt'), K.field('Background', 'None')),
    K.pair(K.select('Auto', { label: '1st line' }), K.num('Offset', '0', 'pt'))]),
  K.section('Options'),
  K.body([K.field('URL', 'https://'),
    K.select('_self', { label: 'Target' }), K.check('Render as HTML', false),
    K.pair(K.field('Variable', ' '), K.field('Restrict', ' '))]),
  K.section('Filters'),
  K.body([P1.list([P1.rec('Property', 'Value', 'head', 124), P1.rec('Drop shadow', 'Blur 4 px · 100 %', 'sel', 124), P1.rec('Glow', 'Blur 6 px · #FFCC00', null, 124)])]),
] : [
  K.section('Paragraph', false),
  K.section('Display'),
  K.body([K.pair(K.num('Opacity', '100', '%'), K.slider())]),
  K.section('Filters', false),
]));

// ---- Frame sound
P1.wave = () => {
  const bars = [];
  for (let i = 0; i < 70; i++) bars.push(rect(2, 4 + ((i * 7) % 11) * 2, '#3F9470', { radius: 1 }));
  return row({ name: 'Waveform', w: 'fill', h: 28, justify: 'SPACE_BETWEEN', pad: [0, 2], fill: 'bg/field', stroke: 'border/subtle', radius: 'radius/control', clip: true }, bars);
};
P1.propsSound = () => P1.panel([
  S.typeHeader('frame', 'Frame 1', 'Layer “music.mp3” · keyframe'),
  K.section('Label', false),
  K.section('Sound'),
  K.body([K.select('music.mp3'),
    K.pair(K.select('Fade in', { label: 'Effect' }), K.select('Stream', { label: 'Sync' })),
    K.pair(K.select('Repeat'), K.num('', '1', 'times', { stepper: true })),
    P1.wave(),
    P1.note('44 kHz · Stereo · 16 bit · 32.4 s · 518.6 kB'),
    K.line([K.btn('Play', { icon: 'play' }), K.btn('Envelope…'), spacer(), K.ibtn('settings', { kind: 'Outlined' })])]),
]);

// ---- Video (instance on the stage, frame video on a video layer)
P1.preview = () => col({ name: 'Video preview', w: 'fill', h: 132, fill: '#3A3550', radius: 'radius/control', clip: true, justify: 'MAX' }, [
  row({ name: 'Transport', w: 'fill', h: 24, pad: [0, 8], gap: 8, fill: '#14161F' },
    [icon('play', 'text/on-accent', 12), rect('fill', 3, '#8088A0', { name: 'Progress', radius: 2 }), txt('00:07 / 00:24', 'Mono Small', '#FFFFFF')])]);
P1.propsVideo = () => P1.panel([
  S.typeHeader('video', 'Video', 'Instance of “intro.mp4”'),
  K.body([P1.preview(), K.line([K.field('Name', 'intro_vid'), K.btn('Swap…')]), P1.note('intro.mp4 · H.264 · 1280×720 · 30 fps · 24.0 s')]),
].concat(P1.pos('96.0', '64.0', '640.0', '360.0'), [
  K.section('Display', false),
  K.section('Playback'),
  K.body([K.select('Embedded', { label: 'Deployment' }),
    K.line([K.check('Autoplay', true), K.check('Loop', false), K.check('Muted', false)]),
    K.select('Minimal overlay', { label: 'Skin' }),
    K.pair(K.select('Fit', { label: 'Scale' }), K.num('Volume', '80', '%')), K.slider()]),
  K.section('Cue points'),
  K.body([P1.list([P1.rec('Name', 'Time · Type', 'head', 132), P1.rec('titleIn', '00:01.500 · Event', 'sel', 132), P1.rec('logoHit', '00:12.000 · Navigation', null, 132)]),
    K.line([K.ibtn('plus'), K.ibtn('minus'), spacer(), K.btn('Edit in timeline', { kind: 'Ghost' })])]),
]));
P1.propsFrameVideo = () => P1.panel([
  S.typeHeader('frame', 'Frame 12', 'Layer “intro.mp4” · video span'),
  K.section('Video'),
  K.body([K.select('intro.mp4'),
    K.line([K.select('Stream', { label: 'Sync' }), K.check('Loop', false), K.check('Mute', false)]),
    S.group('Trim'),
    K.pair(K.num('In', '0', 'ms'), K.num('Out', '24 000', 'ms')),
    K.line([K.btn('Split at playhead'), K.btn('Extract audio')]),
    K.line([K.btn('Reset trim', { kind: 'Ghost' })]),
    S.group('Cue points'),
    P1.list([P1.rec('Name', 'Time', 'head', 72), P1.rec('titleIn', '1 500 ms', 'sel', 72), P1.rec('logoHit', '12 000 ms', null, 72)]),
    K.line([K.ibtn('plus'), K.ibtn('minus')])]),
]);

// ---- Primitives
P1.propsOval = () => P1.panel([S.typeHeader('oval', 'Oval primitive', 'Layer “ball”')].concat(P1.pos('100.0', '100.0', '50.0', '50.0'), [
  K.section('Fill and stroke'),
  K.body([P1.paint('Fill', '#2B5CAD', 100), P1.paint('Stroke', '#000000', 100)].concat(P1.strokeRows('2', false),
    [K.line([K.btn('Custom…', { icon: 'settings' }), spacer(), K.num('Miter', '3', null, { state: 'Disabled', w: 'width/label' })])])),
  K.section('Oval options'),
  K.body([K.pair(P1.st('Start', '90', '°'), P1.st('End', '0', '°')), K.pair(P1.st('Inner', '0', '%'), P1.cell(K.check('Close path', true))),
    K.line([K.btn('Reset', { kind: 'Ghost', icon: 'refresh' })])]),
]));
P1.propsRect = () => P1.panel([S.typeHeader('rect', 'Rectangle primitive', 'Editable corners')].concat(P1.pos('140.0', '96.0', '240.0', '120.0'), [
  K.section('Fill and stroke'),
  K.body([P1.paint('Fill', '#2B5CAD', 100), P1.paint('Stroke', '#000000', 100), K.pair(K.num('Width', '1', 'px', { stepper: true }), K.select('Solid', { label: 'Style' }))]),
  K.section('Rectangle options', true, { action: 'link' }),
  K.body([K.pair(P1.st('↖', '12'), P1.st('↗', '12')), K.pair(P1.st('↙', '12'), P1.st('↘', '12')), K.slider(),
    K.line([K.btn('Reset', { kind: 'Ghost', icon: 'refresh' })])]),
]));

// ---- Drawing tools
P1.shapeArt = i => {
  if (i === 0) return dot(10, 'text/primary');
  if (i === 1 || i === 2) { const e = figma.createEllipse(); e.name = 'Oval'; e.resize(i === 1 ? 12 : 6, i === 1 ? 6 : 12); e.fills = fillOf('text/primary'); return e; }
  if (i === 3) return rect(9, 9, 'text/primary');
  if (i === 4) return rect(12, 4, 'text/primary');
  if (i === 5) return rect(4, 12, 'text/primary');
  const r = rect(14, 2, 'text/primary'); r.rotation = i === 6 ? 45 : -45; return r;
};
P1.SHAPES = ['Round', 'Oval H', 'Oval V', 'Square', 'Rect H', 'Rect V', 'Slash', 'Backslash'];
P1.shapes = sel => row({ name: 'Brush shapes', w: 'fill', gap: 'gap/tool' }, P1.SHAPES.map((s, i) =>
  box({ name: 'Brush shape / ' + s, w: 'size/control', h: 'size/control', radius: 'radius/control', justify: 'CENTER', clip: true,
    fill: i === sel ? 'bg/selected' : 'bg/field', stroke: i === sel ? 'accent/default' : 'border/default' }, [P1.shapeArt(i)])));
P1.propsBrush = () => P1.panel([
  S.typeHeader('brush', 'Brush tool', 'Paints fills · B'),
  K.section('Brush mode'),
  K.body([K.select('Paint normal'), P1.note('Normal · Fills · Behind · Selection · Inside — where paint lands relative to existing shapes.')]),
  K.section('Shape and size'),
  K.body([P1.shapes(0), P1.slide('Size', '12', 'px', '1', '200'), P1.slide('Smoothing', '50', null, '0', '100')]),
  K.section('Pen pressure and tilt'),
  K.body([K.line([icon('pressure'), K.check('Pressure controls size', true)]), K.line([icon('tilt'), K.check('Tilt controls angle', false)])]),
  K.section('Fill'),
  K.body([P1.paint('Fill', '#FFCC00', 100), K.line([K.check('Lock fill', false), spacer(), K.btn('Brush library…')])]),
]);
P1.propsPencil = () => P1.panel([
  S.typeHeader('pencil', 'Pencil tool', 'Draws strokes · Y'),
  K.section('Pencil mode'),
  K.body([K.line([P1.igroup(['pencil-straighten', 'pencil-smooth', 'pencil-ink'], 1), K.t('Smooth', 'text/primary')]),
    P1.note('Straighten turns strokes into lines and arcs, Smooth evens out the curve, Ink keeps the stroke as drawn.'),
    P1.slide('Smoothing', '50', null, '0', '100')]),
  K.section('Stroke'),
  K.body([P1.paint('Stroke', '#000000', 100)].concat(P1.strokeRows('2', true))),
]);
P1.decoShape = label => K.line([row({ name: 'Label', w: 'width/label' }, [K.t(label)]), txt('Default shape', 'Body', 'text/primary', { grow: true }), K.btn('Edit…', { kind: 'Ghost' })]);
P1.propsDeco = () => P1.panel([
  S.typeHeader('tool-deco', 'Deco tool', 'Patterns from symbols'),
  K.section('Drawing effect'),
  K.body([K.select('Vine fill'), P1.decoShape('Leaf'), P1.decoShape('Flower')]),
  K.section('Advanced options'),
  K.body([K.pair(P1.st('Branch', '0', '°'), P1.st('Scale', '100', '%')), K.pair(P1.st('Segment', '6', 'px'), P1.st('Step', '1', 'f')),
    K.line([K.check('Animate pattern', true), K.small('one keyframe per step', null, { grow: true })])]),
]);
P1.propsSpray = () => P1.panel([
  S.typeHeader('tool-spray', 'Spray brush', 'Sprays a symbol or dots'),
  K.section('Symbol'),
  K.body([K.line([K.field('Symbol', 'leaf_mc'), K.btn('Edit…')]), K.check('Use default shape (dots)', false),
    K.pair(P1.st('W', '100', '%'), P1.st('H', '100', '%')),
    K.check('Random scaling', true), K.check('Rotate symbol', true), K.check('Random rotation', true)]),
  K.section('Brush'),
  K.body([K.pair(P1.st('Width', '24', 'px'), P1.st('Height', '24', 'px')), K.pair(P1.st('Angle', '0', '°'), spacer())]),
]);

async function nf3BuildPropsA() {
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Panel / Properties — Text', 'One layout that fits width/inspector in every density, so the v2.25 narrow fallback board is gone. The first section is “Position and size” (R-100); alignment and underline / strike / super / sub are icon groups instead of long selects. Left: text box, opacity stays in the colour field. Right: simple text with its own Display section holding Opacity.',
    [P1.side([P1.propsText(true), P1.propsText(false)])], { noteW: 600 }));
  out.push(nfBoard(page, 'Panel / Properties — Frame sound', 'One-line type header; Effect and Sync share a row, Repeat / Loop sits next to its count. “Sound properties…” is the settings icon button at the end of the action row.',
    [P1.propsSound()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Properties — Video', 'Left: video instance on the stage; right: frame video on a video layer. Playback options are checkboxes, cue points are one row per record, and Display (Opacity, R-100) is a collapsed section of the instance.',
    [P1.side([P1.propsVideo(), P1.propsFrameVideo()])], { noteW: 600 }));
  out.push(nfBoard(page, 'Panel / Properties — Primitives', 'Fill and stroke are one-row colour records instead of two colour wells; constrain proportions and lock corners moved to the link action of their section header; “Stroke hinting” is a checkbox.',
    [P1.side([P1.propsOval(), P1.propsRect()])], { noteW: 600 }));
  out.push(nfBoard(page, 'Panel / Properties — Drawing tools', 'Brush, Pencil, Deco and Spray brush as the Properties panel shows them for the active tool. Every slider sits under its labelled value with the range ends around it; pressure, tilt and hinting are checkboxes; the long Deco option is split into a checkbox and a hint.',
    [P1.side([P1.propsBrush(), P1.propsPencil(), P1.propsDeco(), P1.propsSpray()])], { noteW: 900 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
