// Creates the v3 pages and moves the live icon shelves (NF2 Icon/*) from the v2.25 system page to v3 · Atoms.
async function nf3Setup() {
  const names = ['v3 · Concept', 'v3 · Atoms'];
  const pages = names.map(n => nfPage(n));
  pages.forEach((p, i) => figma.root.insertChild(i, p));
  const atoms = pages[1];
  await atoms.loadAsync();
  const sys = figma.root.children.find(p => p.name.indexOf('v2.25 · System & Components') === 0);
  const moved = [], tops = {};
  if (sys) {
    await sys.loadAsync();
    const map = new Map();
    for (const c of sys.findAllWithCriteria({ types: ['COMPONENT'] })) {
      if (c.name.indexOf('NF2 Icon/') !== 0) continue;
      let t = c; while (t.parent && t.parent.type !== 'PAGE') t = t.parent;
      map.set(t.id, { t: t, n: (map.get(t.id) ? map.get(t.id).n : 0) + 1 });
    }
    let y = 0;
    for (const e of map.values()) {
      tops[e.t.name] = e.n;
      atoms.appendChild(e.t); e.t.x = -e.t.width - 400; e.t.y = y; y += e.t.height + 200;
      moved.push({ id: e.t.id, name: e.t.name, icons: e.n });
    }
  }
  const boolDefs = { 'show/tool-20': [true, false, false], 'show/tool-16': [false, true, true] };
  const ids = ['Normal', 'Compact', 'Classic Flash'].map(n => NF.densColl.modes.find(m => m.name === n).modeId);
  for (const name of Object.keys(boolDefs)) {
    let v = NF.v[name];
    if (!v) { v = figma.variables.createVariable(name, NF.densColl, 'BOOLEAN'); NF.v[name] = v; }
    ids.forEach((id, i) => v.setValueForMode(id, boolDefs[name][i]));
  }
  return { pages: pages.map(p => ({ id: p.id, name: p.name })), moved, tops };
}
