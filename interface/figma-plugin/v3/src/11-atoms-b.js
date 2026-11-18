// NF3 atoms, part B: navigation, lists, display, tools.
function nf3One(slot, name, node) { node.name = 'NF3/' + name; slot.appendChild(node); NF.atoms[name] = node; return node; }
function nf3AtomsB() {
  const shelf = nf3Shelf(), made = [];
  made.push(nf3Slot(shelf, 'Tab', 'Panel and dock tabs. The 2 pt accent line is an inside stroke, so switching tabs never moves the label.', slot => {
    const vs = [];
    for (const sel of ['No', 'Yes']) {
      const ic = icon('layers', sel === 'Yes' ? 'accent/default' : 'text/secondary'); ic.visible = false;
      vs.push({ props: { Selected: sel }, node: C({ h: 'size/tab', pad: [0, 'pad/panel'], gap: 'gap/inline', stroke: sel === 'Yes' ? 'accent/default' : null, sides: [0, 0, 2, 0] },
        [ic, txt('Properties', 'Body Strong', sel === 'Yes' ? 'text/primary' : 'text/secondary', { name: 'Label' })]) });
    }
    const set = makeSet('Tab', vs, slot, 320);
    linkText(set, 'Label', 'Label', 'Properties'); linkBool(set, 'Show icon', 'Icon', false); linkSwap(set, 'Icon', 'Icon', iconComp('layers'));
    return set;
  }));
  made.push(nf3Slot(shelf, 'Section Header', 'Overline caption with a 12 px chevron. The section body starts gap/row below it.', slot => {
    const vs = [];
    for (const open of ['Yes', 'No']) {
      const act = icon('more', 'text/tertiary'); act.name = 'Action'; act.visible = false;
      vs.push({ props: { Open: open }, node: C({ w: 264, h: 'size/section', pad: [0, 'pad/panel'], gap: 4 },
        [icon(open === 'Yes' ? 'chevdown' : 'chevright', 'text/tertiary', 12), txt('Position and size', 'Overline', 'text/secondary', { name: 'Title', grow: true }), act]) });
    }
    const set = makeSet('Section Header', vs, slot, 620);
    linkText(set, 'Title', 'Title', 'Position and size'); linkBool(set, 'Action', 'Action', false); linkSwap(set, 'Action icon', 'Action', iconComp('more'));
    return set;
  }));
  made.push(nf3Slot(shelf, 'Dialog Header', null, slot => {
    const close = inst('Icon Button', { Icon: iconComp('x') }); close.name = 'Close';
    return nf3One(slot, 'Dialog Header', C({ w: 480, h: 'size/dialog-header', pad: [0, 'pad/panel', 0, 'pad/dialog'], gap: 8, stroke: 'border/subtle', sides: [0, 0, 1, 0] },
      [txt('Dialog title', 'Dialog Title', 'text/primary', { name: 'Title', grow: true }), close]));
  }));
  const dh = NF.atoms['Dialog Header']; dh.addComponentProperty('Title', 'TEXT', 'Dialog title');
  { const key = Object.keys(dh.componentPropertyDefinitions)[0]; dh.findOne(n => n.name === 'Title').componentPropertyReferences = { characters: key }; }
  made.push(nf3Slot(shelf, 'Menu Item', 'Row = size/row. The check column appears only in menus that have checkable items.', slot => {
    const vs = [];
    for (const state of ['Default', 'Hover', 'Disabled']) {
      const fg = state === 'Disabled' ? 'text/disabled' : 'text/primary';
      const chk = box({ name: 'Check column', w: 16, h: 16, justify: 'CENTER' }, [(() => { const c = icon('check', fg, 12); c.name = 'Check'; return c; })()]); chk.visible = false;
      const key = txt('⌘K', 'Small', state === 'Disabled' ? 'text/disabled' : 'text/tertiary', { name: 'Shortcut' }); key.visible = false;
      const sub = icon('chevright', 'text/tertiary', 12); sub.name = 'Submenu'; sub.visible = false;
      vs.push({ props: { State: state }, node: C({ w: 'width/menu', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/check', fill: state === 'Hover' ? 'bg/hover' : null },
        [chk, txt('Menu item', 'Body', fg, { name: 'Label', grow: true }), key, sub]) });
    }
    const set = makeSet('Menu Item', vs, slot, 720);
    linkText(set, 'Label', 'Label', 'Menu item'); linkBool(set, 'Check column', 'Check column', false); linkBool(set, 'Checked', 'Check', true);
    linkText(set, 'Shortcut', 'Shortcut', '⌘K'); linkBool(set, 'Show shortcut', 'Shortcut', false); linkBool(set, 'Submenu', 'Submenu', false);
    return set;
  }));
  made.push(nf3Slot(shelf, 'Menu Separator', null, slot => nf3One(slot, 'Menu Separator', C({ w: 'width/menu', h: 'size/menu-separator', pad: [0, 'pad/field'] }, [rect('fill', 1, 'border/subtle')]))));
  made.push(nf3Slot(shelf, 'Tree Row', 'Library, Movie Explorer, sidebars. 14 pt indent per level.', slot => {
    const vs = [];
    for (const level of ['0', '1', '2']) for (const state of ['Default', 'Hover', 'Selected']) {
      const ch = icon('chevdown', 'text/tertiary', 12); ch.name = 'Chevron';
      const meta = txt('4', 'Small', 'text/tertiary', { name: 'Meta' }); meta.visible = false;
      vs.push({ props: { Level: level, State: state }, node: C({ w: 232, h: 'size/row', pad: [0, 'pad/field', 0, 6 + Number(level) * 14], gap: 4, fill: state === 'Hover' ? 'bg/hover' : state === 'Selected' ? 'bg/selected' : null },
        [ch, icon('folder'), txt('Library item', 'Body', 'text/primary', { name: 'Label', grow: true }), meta]) });
    }
    const set = makeSet('Tree Row', vs, slot, 800);
    linkText(set, 'Label', 'Label', 'Library item'); linkBool(set, 'Chevron', 'Chevron', true); linkSwap(set, 'Chevron icon', 'Chevron', iconComp('chevdown', 12));
    linkBool(set, 'Show icon', 'Icon', true); linkSwap(set, 'Icon', 'Icon', iconComp('folder')); linkText(set, 'Meta', 'Meta', '4'); linkBool(set, 'Show meta', 'Meta', false);
    return set;
  }));
  made.push(nf3Slot(shelf, 'Badge', null, slot => {
    const tones = { Neutral: ['bg/hover', 'text/secondary'], Accent: ['accent/subtle', 'accent/default'], Success: ['bg/hover', 'success/default'], Warning: ['warning/subtle', 'warning/default'], Danger: ['danger/subtle', 'danger/default'] };
    const vs = Object.keys(tones).map(t => ({ props: { Tone: t }, node: C({ h: 'size/badge', pad: [0, 6], radius: 99, fill: tones[t][0], justify: 'CENTER' }, [txt('Badge', 'Small Strong', tones[t][1], { name: 'Label' })]) }));
    const set = makeSet('Badge', vs, slot, 420); linkText(set, 'Label', 'Label', 'Badge'); return set;
  }));
  made.push(nf3Slot(shelf, 'Kbd', null, slot => nf3One(slot, 'Kbd', C({ h: 'size/badge', pad: [0, 5], radius: 3, fill: 'bg/raised', stroke: 'border/default', justify: 'CENTER' }, [txt('F8', 'Mono Small', 'text/secondary', { name: 'Label' })]))));
  { const k = NF.atoms['Kbd']; const key = k.addComponentProperty('Label', 'TEXT', 'F8'); k.findOne(n => n.name === 'Label').componentPropertyReferences = { characters: key }; }
  made.push(nf3Slot(shelf, 'Tooltip', null, slot => {
    const sc = txt('V', 'Mono Small', 'text/inverse', { name: 'Shortcut' }); sc.opacity = 0.7;
    return nf3One(slot, 'Tooltip', C({ pad: [4, 8], gap: 8, radius: 'radius/control', fill: 'tooltip/bg' }, [txt('Selection tool', 'Small', 'text/inverse', { name: 'Label' }), sc]));
  }));
  { const k = NF.atoms['Tooltip']; const a = k.addComponentProperty('Label', 'TEXT', 'Selection tool'); k.findOne(n => n.name === 'Label').componentPropertyReferences = { characters: a };
    const b = k.addComponentProperty('Shortcut', 'TEXT', 'V'); const c = k.addComponentProperty('Show shortcut', 'BOOLEAN', true); k.findOne(n => n.name === 'Shortcut').componentPropertyReferences = { characters: b, visible: c }; }
  made.push(nf3Slot(shelf, 'Tool Button', 'Side = size/tool. Normal shows the 20 px master, Compact and Classic Flash the 16 px master (density booleans show/tool-20, show/tool-16).', slot => {
    const vs = [];
    for (const state of ['Default', 'Hover', 'Selected', 'Disabled']) {
      const fg = state === 'Selected' ? 'accent/default' : state === 'Disabled' ? 'text/disabled' : 'text/secondary';
      const i20 = icon('tool-select', fg, 20); i20.name = 'Icon 20'; i20.setBoundVariable('visible', V('show/tool-20'));
      const i16 = icon('select', fg, 16); i16.name = 'Icon 16'; i16.setBoundVariable('visible', V('show/tool-16'));
      const tri = figma.createPolygon(); tri.pointCount = 3; tri.resize(5, 4); tri.rotation = 180; tri.fills = fillOf(fg); tri.name = 'Flyout'; tri.visible = false;
      const n = C({ w: 'size/tool', h: 'size/tool', radius: 'radius/control', justify: 'CENTER', fill: state === 'Hover' ? 'bg/hover' : state === 'Selected' ? 'bg/selected' : null }, [i20, i16, tri]);
      tri.layoutPositioning = 'ABSOLUTE'; tri.constraints = { horizontal: 'MAX', vertical: 'MAX' }; tri.x = n.width - 2; tri.y = n.height - 2;
      vs.push({ props: { State: state }, node: n });
    }
    const set = makeSet('Tool Button', vs, slot, 240);
    linkSwap(set, 'Icon 20', 'Icon 20', iconComp('tool-select', 20)); linkSwap(set, 'Icon 16', 'Icon 16', iconComp('select', 16)); linkBool(set, 'Flyout', 'Flyout', false);
    return set;
  }));
  made.push(nf3Slot(shelf, 'Inline Message', 'Icon + text in the tone colour; never colour alone.', slot => {
    const tones = { Info: ['info', 'text/secondary'], Warning: ['warning', 'warning/default'], Error: ['warning', 'danger/default'], Success: ['check', 'success/default'] };
    const vs = Object.keys(tones).map(t => ({ props: { Tone: t }, node: C({ w: 240, gap: 'gap/inline', align: 'MIN' }, [icon(tones[t][0], tones[t][1]), txt('Message text that may wrap to a second line.', 'Small', tones[t][1], { name: 'Text', grow: true, wrap: true })]) }));
    const set = makeSet('Inline Message', vs, slot, 560); linkText(set, 'Text', 'Text', 'Message text that may wrap to a second line.'); return set;
  }));
  made.push(nf3Slot(shelf, 'Swatch', 'Colour chip; the fill is overridden per instance.', slot => nf3One(slot, 'Swatch', C({ w: 'size/swatch', h: 'size/swatch', radius: 3, fill: '#3B6FB6', stroke: 'border/default' }, []))));
  made.push(nf3Slot(shelf, 'Progress', null, slot => nf3One(slot, 'Progress', C({ w: 200, h: 4, radius: 2, fill: 'border/strong', clip: true }, [rect(120, 4, 'accent/default')]))));
  return made.map(s => ({ id: s.id, name: s.name, variants: s.type === 'COMPONENT_SET' ? s.children.length : 1 }));
}
