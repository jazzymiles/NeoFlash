// NF3 atoms, part A: shelf, buttons, fields, toggles.
const C = (o, kids) => box(Object.assign({ component: true }, o), kids);
function nf3Shelf() {
  let s = NF.atomsPage.findChild(n => n.name === 'NF3 / Atoms');
  if (!s) {
    s = col({ name: 'NF3 / Atoms', gap: 32, pad: 48, fill: 'bg/app', w: 1680 });
    NF.atomsPage.appendChild(s); s.x = 0; s.y = 0;
    add(s, [txt('NF3 · Atoms', 'Heading', 'text/primary'),
      txt('Every atom takes its heights, paddings, gaps and radii from the “NF3 · Density” collection (Normal / Compact / Classic Flash) and its colours from “NF2 · Tokens” (Dark / Light / Mid · Neutral Gray / Dark · Graphite). Set both modes on a frame or page to preview a combination.', 'Dialog Body', 'text/secondary', { w: 1100 })]);
  }
  return s;
}
function nf3Slot(shelf, title, note, build) {
  const name = 'Atom / ' + title;
  const old = shelf.findChild(n => n.name === name); if (old) old.remove();
  const slot = col({ name: name, gap: 8 }, [txt(title, 'Title', 'text/primary'), note ? txt(note, 'Small', 'text/tertiary', { w: 1100 }) : null]);
  shelf.appendChild(slot);
  const set = build(slot);
  return set;
}
function nf3AtomsA() {
  const shelf = nf3Shelf(), made = [];
  // Button
  made.push(nf3Slot(shelf, 'Button', 'One Primary per surface. Secondary for everything else, Ghost inside toolbars and footers, Danger for destructive actions.', slot => {
    const kinds = { Secondary: ['bg/raised', 'border/default', 'text/primary'], Primary: ['accent/default', null, 'text/on-accent'], Ghost: [null, null, 'text/secondary'], Danger: [null, 'danger/default', 'danger/default'] };
    const vs = [];
    for (const kind of Object.keys(kinds)) for (const state of ['Default', 'Hover', 'Focus', 'Disabled']) {
      let fill = kinds[kind][0], stroke = kinds[kind][1], fg = kinds[kind][2];
      if (state === 'Hover') fill = kind === 'Primary' ? 'accent/hover' : kind === 'Danger' ? 'danger/subtle' : 'bg/hover';
      if (state === 'Disabled') { fg = 'text/disabled'; if (kind === 'Primary') fill = 'bg/hover'; if (stroke) stroke = 'border/subtle'; }
      if (state === 'Focus') stroke = 'border/focus';
      const ic = icon('plus', fg); ic.visible = false;
      vs.push({ props: { Kind: kind, State: state }, node: C({ h: 'size/control', pad: [0, 'pad/button'], gap: 'gap/inline', radius: 'radius/control', fill: fill, stroke: stroke, strokeWeight: state === 'Focus' ? 2 : 1, justify: 'CENTER' },
        [ic, txt('Button', 'Body Strong', fg, { name: 'Label' })]) });
    }
    const set = makeSet('Button', vs, slot, 520);
    linkText(set, 'Label', 'Label', 'Button'); linkBool(set, 'Show icon', 'Icon', false); linkSwap(set, 'Icon', 'Icon', iconComp('plus'));
    return set;
  }));
  // Icon Button
  made.push(nf3Slot(shelf, 'Icon Button', 'Square, side = size/control. Always has a tooltip with its shortcut.', slot => {
    const vs = [];
    for (const kind of ['Ghost', 'Outlined']) for (const state of ['Default', 'Hover', 'Selected', 'Disabled']) {
      const fg = state === 'Selected' ? 'accent/default' : state === 'Disabled' ? 'text/disabled' : 'text/secondary';
      const fill = state === 'Hover' ? 'bg/hover' : state === 'Selected' ? 'bg/selected' : kind === 'Outlined' ? 'bg/raised' : null;
      vs.push({ props: { Kind: kind, State: state }, node: C({ w: 'size/control', h: 'size/control', radius: 'radius/control', fill: fill, stroke: kind === 'Outlined' ? 'border/default' : null, justify: 'CENTER' }, [icon('more', fg)]) });
    }
    const set = makeSet('Icon Button', vs, slot, 240);
    linkSwap(set, 'Icon', 'Icon', iconComp('more'));
    return set;
  }));
  // Field
  made.push(nf3Slot(shelf, 'Field', 'Inline label prefix, value, optional unit and stepper. Numeric fields default to width/num; in the inspector grid they fill their column.', slot => {
    const vs = [];
    for (const state of ['Default', 'Hover', 'Focus', 'Error', 'Disabled']) {
      const dis = state === 'Disabled';
      const stroke = { Default: 'border/default', Hover: 'border/strong', Focus: 'border/focus', Error: 'danger/default', Disabled: 'border/subtle' }[state];
      const unit = txt('px', 'Body', 'text/tertiary', { name: 'Unit' }); unit.visible = false;
      const step = col({ name: 'Stepper', gap: -4 }, [icon('chevup', 'text/tertiary', 12), icon('chevdown', 'text/tertiary', 12)]); step.visible = false;
      vs.push({ props: { State: state }, node: C({ w: 120, h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/control', fill: dis ? 'bg/panel' : 'bg/field', stroke: stroke },
        [txt('X', 'Body', dis ? 'text/disabled' : 'text/tertiary', { name: 'Label' }), txt('640', 'Body', dis ? 'text/disabled' : 'text/primary', { name: 'Value', grow: true }), unit, step]) });
    }
    const set = makeSet('Field', vs, slot, 720);
    linkText(set, 'Label', 'Label', 'X'); linkBool(set, 'Show label', 'Label', true); linkText(set, 'Value', 'Value', '640');
    linkText(set, 'Unit', 'Unit', 'px'); linkBool(set, 'Show unit', 'Unit', false); linkBool(set, 'Stepper', 'Stepper', false);
    return set;
  }));
  // Select
  made.push(nf3Slot(shelf, 'Select', 'Optional label prefix. Width follows the content class of its form row, never the whole dialog.', slot => {
    const vs = [];
    for (const state of ['Default', 'Hover', 'Open', 'Disabled']) {
      const dis = state === 'Disabled';
      const stroke = { Default: 'border/default', Hover: 'border/strong', Open: 'border/focus', Disabled: 'border/subtle' }[state];
      const lab = txt('Style', 'Body', dis ? 'text/disabled' : 'text/tertiary', { name: 'Label' }); lab.visible = false;
      vs.push({ props: { State: state }, node: C({ w: 140, h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/control', fill: dis ? 'bg/panel' : 'bg/field', stroke: stroke },
        [lab, txt('Normal', 'Body', dis ? 'text/disabled' : 'text/primary', { name: 'Value', grow: true }), icon('chevdown', dis ? 'text/disabled' : 'text/secondary', 12)]) });
    }
    const set = makeSet('Select', vs, slot, 680);
    linkText(set, 'Label', 'Label', 'Style'); linkBool(set, 'Show label', 'Label', false); linkText(set, 'Value', 'Value', 'Normal');
    return set;
  }));
  // Search
  made.push(nf3Slot(shelf, 'Search', null, slot => {
    const vs = [];
    for (const state of ['Default', 'Focus']) {
      const clear = icon('x', 'text/tertiary', 12); clear.name = 'Clear'; clear.visible = false;
      vs.push({ props: { State: state }, node: C({ w: 200, h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/control', fill: 'bg/field', stroke: state === 'Focus' ? 'border/focus' : 'border/default' },
        [icon('search', 'text/tertiary'), txt('Search', 'Body', 'text/tertiary', { name: 'Value', grow: true }), clear]) });
    }
    const set = makeSet('Search', vs, slot, 480);
    linkText(set, 'Value', 'Value', 'Search'); linkBool(set, 'Clear', 'Clear', false);
    return set;
  }));
  // Checkbox
  made.push(nf3Slot(shelf, 'Checkbox', 'Form options are checkboxes. Row = size/toggle, box = size/check.', slot => {
    const vs = [];
    for (const value of ['Off', 'On', 'Mixed']) for (const state of ['Default', 'Disabled']) {
      const dis = state === 'Disabled', on = value !== 'Off';
      const mark = on ? icon(value === 'On' ? 'check' : 'minus', dis ? 'text/disabled' : 'text/on-accent', 12) : null;
      const b = box({ name: 'Box', w: 'size/check', h: 'size/check', radius: 'radius/check', justify: 'CENTER', clip: true,
        fill: on ? (dis ? 'bg/hover' : 'accent/default') : (dis ? 'bg/panel' : 'bg/field'), stroke: on ? null : (dis ? 'border/subtle' : 'border/strong') }, mark ? [mark] : []);
      vs.push({ props: { Value: value, State: state }, node: C({ h: 'size/toggle', gap: 'gap/inline' }, [b, txt('Option', 'Body', dis ? 'text/disabled' : 'text/primary', { name: 'Label' })]) });
    }
    const set = makeSet('Checkbox', vs, slot, 360);
    linkText(set, 'Label', 'Label', 'Option'); linkBool(set, 'Show label', 'Label', true);
    return set;
  }));
  // Radio
  made.push(nf3Slot(shelf, 'Radio', null, slot => {
    const vs = [];
    for (const value of ['Off', 'On']) for (const state of ['Default', 'Disabled']) {
      const dis = state === 'Disabled', on = value === 'On';
      const b = box({ name: 'Ring', w: 'size/check', h: 'size/check', radius: 99, justify: 'CENTER', fill: dis ? 'bg/panel' : 'bg/field',
        stroke: on ? (dis ? 'border/strong' : 'accent/default') : (dis ? 'border/subtle' : 'border/strong'), strokeWeight: on ? 1.5 : 1 }, on ? [dot(6, dis ? 'text/disabled' : 'accent/default')] : []);
      vs.push({ props: { Value: value, State: state }, node: C({ h: 'size/toggle', gap: 'gap/inline' }, [b, txt('Option', 'Body', dis ? 'text/disabled' : 'text/primary', { name: 'Label' })]) });
    }
    const set = makeSet('Radio', vs, slot, 360);
    linkText(set, 'Label', 'Label', 'Option');
    return set;
  }));
  // Switch
  made.push(nf3Slot(shelf, 'Switch', 'Only for an application-level state that applies at once (Autosave, Snapping). Never inside a form with OK / Cancel.', slot => {
    const vs = [];
    for (const value of ['Off', 'On']) for (const state of ['Default', 'Disabled']) {
      const dis = state === 'Disabled', on = value === 'On';
      const tr = box({ name: 'Track', w: 'size/switch-w', h: 'size/switch-h', radius: 99, pad: [0, 2], justify: on ? 'MAX' : 'MIN',
        fill: dis ? 'bg/hover' : on ? 'accent/default' : 'border/strong' }, [dot('size/knob', dis ? 'text/disabled' : 'text/on-accent')]);
      vs.push({ props: { Value: value, State: state }, node: C({ h: 'size/toggle', gap: 'gap/inline' }, [tr, txt('Autosave', 'Body', dis ? 'text/disabled' : 'text/primary', { name: 'Label' })]) });
    }
    const set = makeSet('Switch', vs, slot, 360);
    linkText(set, 'Label', 'Label', 'Autosave'); linkBool(set, 'Show label', 'Label', true);
    return set;
  }));
  // Slider
  made.push(nf3Slot(shelf, 'Slider', 'Rail border/strong, filled part and thumb ring accent. Usually paired with a width/num Field.', slot => {
    const vs = [];
    for (const state of ['Default', 'Disabled']) {
      const dis = state === 'Disabled';
      vs.push({ props: { State: state }, node: C({ w: 140, h: 'size/toggle', gap: 0 },
        [rect(84, 2, dis ? 'border/strong' : 'accent/default', { radius: 1 }), dot('size/slider-thumb', 'bg/raised', { stroke: dis ? 'border/strong' : 'accent/default', strokeWeight: 1.5 }), rect('fill', 2, 'border/strong', { radius: 1 })]) });
    }
    return makeSet('Slider', vs, slot, 360);
  }));
  // Segment
  made.push(nf3Slot(shelf, 'Segment', 'Placed inside a Segmented container (size/control high, 2 pt inset).', slot => {
    const vs = [];
    for (const sel of ['No', 'Yes'])
      vs.push({ props: { Selected: sel }, node: C({ h: 20, pad: [0, 'pad/button'], radius: 'radius/check', justify: 'CENTER', fill: sel === 'Yes' ? 'bg/raised' : null, stroke: sel === 'Yes' ? 'border/default' : null },
        [txt('Segment', 'Body Strong', sel === 'Yes' ? 'text/primary' : 'text/secondary', { name: 'Label' })]) });
    const set = makeSet('Segment', vs, slot, 240);
    linkText(set, 'Label', 'Label', 'Segment');
    return set;
  }));
  return made.map(s => ({ id: s.id, name: s.name, variants: s.children.length }));
}
