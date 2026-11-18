// NF3 kit — short constructors used by every surface script.
const K = {};
// Icon lookup that reports an unknown name (the atom would silently keep its default icon).
function iconReq(name, size) { const c = iconComp(name, size); if (!c) NF.warn.push('icon ' + (size || 16) + '/' + name); return c; }
K.btn = (label, o) => {
  o = o || {};
  const i = inst('Button', { Kind: o.kind || 'Secondary', State: o.state || 'Default', Label: label, 'Show icon': !!o.icon, Icon: o.icon ? iconReq(o.icon) : null }, o);
  const fg = o.state === 'Disabled' ? 'text/disabled' : o.kind === 'Primary' ? 'text/on-accent' : o.kind === 'Danger' ? 'danger/default' : null;
  if (o.icon) tint(i.findOne(x => x.type === 'INSTANCE'), fg || 'text/primary', iconReq(o.icon));
  return i;
};
K.ibtn = (ic, o) => {
  o = o || {};
  const i = inst('Icon Button', { Kind: o.kind || 'Ghost', State: o.state || 'Default', Icon: iconReq(ic, o.size) }, o);
  const fg = o.state === 'Selected' ? 'accent/default' : o.state === 'Disabled' ? 'text/disabled' : o.color;
  tint(i.findOne(x => x.type === 'INSTANCE'), fg || 'text/secondary', iconReq(ic, o.size));
  return i;
};
K.field = (label, value, o) => { o = o || {}; return inst('Field', { State: o.state || 'Default', Label: label || 'X', 'Show label': !!label, Value: String(value), Unit: o.unit || 'px', 'Show unit': !!o.unit, Stepper: !!o.stepper }, { w: o.w || 'fill', name: o.name }); };
K.num = (label, value, unit, o) => K.field(label, value, Object.assign({ unit: unit }, o || {}));
K.select = (value, o) => { o = o || {}; return inst('Select', { State: o.state || 'Default', Label: o.label || 'Style', 'Show label': !!o.label, Value: value }, { w: o.w || 'fill' }); };
K.search = (ph, o) => { o = o || {}; return inst('Search', { State: o.state || 'Default', Value: ph, Clear: !!o.clear }, { w: o.w || 'fill' }); };
K.check = (label, on, o) => { o = o || {}; return inst('Checkbox', { Value: on === true ? 'On' : on === 'mixed' ? 'Mixed' : 'Off', State: o.state || 'Default', Label: label || 'Option', 'Show label': !!label }, o); };
K.radio = (label, on, o) => { o = o || {}; return inst('Radio', { Value: on ? 'On' : 'Off', State: o.state || 'Default', Label: label }, o); };
K.sw = (label, on, o) => { o = o || {}; return inst('Switch', { Value: on ? 'On' : 'Off', State: o.state || 'Default', Label: label || 'Autosave', 'Show label': !!label }, o); };
K.slider = o => { o = o || {}; return inst('Slider', { State: o.state || 'Default' }, { w: o.w || 'fill' }); };
K.seg = (labels, sel, o) => {
  o = o || {};
  const f = row({ name: 'Segmented', h: 'size/control', w: o.w, pad: 2, radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' }, []);
  labels.forEach((l, i) => { const s = inst('Segment', { Selected: i === sel ? 'Yes' : 'No', Label: l }); markGrow(s, !!o.w, true); add(f, s); });
  return f;
};
K.tab = (label, sel, ic) => inst('Tab', { Selected: sel ? 'Yes' : 'No', Label: label, 'Show icon': !!ic, Icon: ic ? iconReq(ic) : null });
K.section = (title, open, o) => inst('Section Header', { Open: open === false ? 'No' : 'Yes', Title: title, Action: !!(o && o.action), 'Action icon': o && o.action ? iconReq(o.action) : null }, { w: 'fill' });
K.dlgHeader = title => inst('Dialog Header', { Title: title }, { w: 'fill' });
K.mi = (label, o) => { o = o || {}; return inst('Menu Item', { State: o.state || 'Default', Label: label, 'Check column': o.check != null, Checked: !!o.check, Shortcut: o.key || '⌘K', 'Show shortcut': !!o.key, Submenu: !!o.sub }, { w: 'fill' }); };
K.msep = () => inst('Menu Separator', null, { w: 'fill' });
K.tree = (label, o) => { o = o || {}; return inst('Tree Row', { Level: String(o.level || 0), State: o.state || 'Default', Label: label, Chevron: o.chev != null, 'Chevron icon': iconReq(o.chev === false ? 'chevright' : 'chevdown', 12), 'Show icon': o.icon !== false, Icon: iconReq(o.icon || 'folder'), Meta: o.meta || '4', 'Show meta': !!o.meta }, { w: 'fill' }); };
K.badge = (label, tone) => inst('Badge', { Tone: tone || 'Neutral', Label: label });
K.kbd = label => inst('Kbd', { Label: label });
K.tip = (label, key) => inst('Tooltip', { Label: label, Shortcut: key || 'V', 'Show shortcut': !!key });
K.msg = (text, tone, o) => inst('Inline Message', { Tone: tone || 'Info', Text: text }, { w: (o && o.w) || 'fill' });
K.swatch = hex => { const i = inst('Swatch'); i.fills = [rawPaint(hex)]; return i; };
K.progress = o => inst('Progress', null, { w: (o && o.w) || 'fill' });
K.tool = (name, state, flyout) => {
  const c20 = iconReq(name.indexOf('tool-') === 0 || name.indexOf('color-') === 0 ? name : 'tool-' + name, 20);
  const c16 = iconComp(name.replace('tool-', ''), 16) || c20;
  const i = inst('Tool Button', { State: state || 'Default', 'Icon 20': c20, 'Icon 16': c16, Flyout: !!flyout });
  const fg = state === 'Selected' ? 'accent/default' : state === 'Disabled' ? 'text/disabled' : 'text/secondary';
  const nested = i.children.filter(x => x.type === 'INSTANCE');
  nested.forEach((n, k) => { tint(n, fg, k === 0 ? c20 : c16); n.setBoundVariable('visible', V(k === 0 ? 'show/tool-20' : 'show/tool-16')); });
  return i;
};
K.t = (s, color, style, o) => txt(s, style || 'Body', color || 'text/secondary', o);
K.small = (s, color, o) => txt(s, 'Small', color || 'text/tertiary', o);
K.pair = (a, b, o) => row(Object.assign({ name: 'Pair', gap: 'gap/control', w: 'fill' }, o || {}), [a, b]);
K.line = (kids, o) => row(Object.assign({ name: 'Line', gap: 'gap/control', w: 'fill', minH: 'size/control' }, o || {}), kids);
K.body = (kids, o) => col(Object.assign({ name: 'Section body', w: 'fill', gap: 'gap/row', pad: [0, 'pad/panel', 'gap/section', 'pad/panel'] }, o || {}), kids);
K.formRow = (label, kids, o) => row(Object.assign({ name: 'Form row', gap: 'gap/control', w: 'fill', minH: 'size/control' }, o || {}),
  [row({ name: 'Label', w: 'width/dialog-label' }, [txt(label, 'Body', 'text/secondary')])].concat(kids));
K.caption = s => txt(s, 'Title', 'text/primary');
// Panel shell: tabs header + scrolling body. tabs: ['Properties', 'Transform'], sel index.
K.panel = (name, o, kids) => {
  o = o || {};
  const head = row({ name: 'Panel header', w: 'fill', h: 'size/tab', pad: [0, 4, 0, 0], stroke: 'border/subtle', sides: [0, 0, 1, 0] },
    (o.tabs || [name]).map((t, i) => K.tab(t, i === (o.sel || 0))).concat([spacer(), o.collapse ? K.ibtn('chevup') : null, K.ibtn('more')]));
  return col({ name: 'Panel / ' + name, w: o.w || 'width/inspector', h: o.h, fill: 'bg/panel', stroke: 'border/subtle', clip: true }, [head].concat(kids || []));
};
K.dialog = (title, o, body, footer) => {
  o = o || {};
  return col({ name: 'Dialog / ' + title, w: o.w || 'width/dialog-m', h: o.h, fill: 'bg/panel', stroke: 'border/default', radius: 'radius/panel', shadow: 'dialog', clip: true },
    [K.dlgHeader(title)].concat(body || []).concat(footer ? [row({ name: 'Footer', w: 'fill', pad: ['pad/panel', 'pad/dialog'], gap: 'gap/control', stroke: 'border/subtle', sides: [1, 0, 0, 0] }, footer)] : []));
};
K.menu = (name, items, o) => col(Object.assign({ name: 'Menu / ' + name, w: 'width/menu', pad: 'pad/menu', fill: 'bg/raised', stroke: 'border/default', radius: 'radius/panel', shadow: 'popover' }, o || {}), items);
// Board: a titled, themed frame on a reference page.
K.board = (title, note, kids, o) => col(Object.assign({ name: title, gap: 24, pad: 40, fill: 'bg/app' }, o || {}),
  [col({ name: 'Board title', gap: 6 }, [txt(title, 'Heading', 'text/primary'), note ? txt(note, 'Dialog Body', 'text/secondary', { w: (o && o.noteW) || 900 }) : null])].concat(kids));
