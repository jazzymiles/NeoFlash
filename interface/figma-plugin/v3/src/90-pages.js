// NF3 reference pages: the master page is built by the surface builders; the other eleven theme × density pages
// are copies of it with two explicit variable modes.
const NF_DENSITIES = ['Normal', 'Compact', 'Classic Flash'];
const NF_THEMES = [['Light', 'Light'], ['Dark', 'Dark'], ['Mid · Neutral Gray', 'Mid Neutral Gray'], ['Dark · Graphite', 'Dark Graphite']];
function nfPageName(density, themeLabel) { return 'v3 · ' + density + ' · ' + themeLabel; }
// Clone the master into one combination (skips the master itself). Call once per run step: page clones are heavy.
async function nf3ClonePage(density, theme) {
  const t = NF_THEMES.find(x => x[0] === theme || x[1] === theme);
  const name = nfPageName(density, t[1]);
  if (name === NF_MASTER) return null;
  const master = figma.root.children.find(p => p.name === NF_MASTER);
  await master.loadAsync();
  const old = figma.root.children.find(p => p.name === name); if (old) old.remove();
  const c = master.clone(); c.name = name; await c.loadAsync();
  nfMode(c, t[0], density);
  return { id: c.id, name: c.name, boards: c.children.length };
}
// Final page order: concept, atoms, the twelve reference pages, builder source, then everything else (archives).
function nf3OrderPages() {
  const names = ['v3 · Concept', NF_ATOMS_PAGE];
  for (const d of NF_DENSITIES) for (const t of NF_THEMES) names.push(nfPageName(d, t[1]));
  names.push('v3 · Builder source');
  let i = 0;
  for (const n of names) { const p = figma.root.children.find(x => x.name === n); if (p) { figma.root.insertChild(i, p); i++; } }
  return figma.root.children.map(p => p.name);
}
