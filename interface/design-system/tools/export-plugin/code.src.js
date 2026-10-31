// NeoFlash Export — sends PNG/SVG exports and a JSON dump of the design system
// to the local receiver (tools/receiver.py), which writes them into design-system/.
// ITEMS is injected by build.py from exports/manifest.json.
const ITEMS = /*ITEMS*/[];

figma.showUI(__html__, { width: 380, height: 220 });

const pending = {};
const post = (path, bytes, text) => new Promise(resolve => {
  pending[path] = resolve;
  figma.ui.postMessage({ type: 'file', path, bytes, text });
});
// The UI encodes text as UTF-8 (the plugin sandbox has no TextEncoder).
const postText = (path, text) => post(path, null, text);
const status = (text) => figma.ui.postMessage({ type: 'status', text });

const dec = s => (s || '').replace(/&quot;/g, '"').replace(/&amp;/g, '&').replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>');
const hex = c => '#' + [c.r, c.g, c.b].map(x => Math.round(x * 255).toString(16).padStart(2, '0')).join('') + (c.a !== undefined && c.a < 0.999 ? Math.round(c.a * 255).toString(16).padStart(2, '0') : '');

async function value(v) {
  if (v && v.type === 'VARIABLE_ALIAS') { const t = await figma.variables.getVariableByIdAsync(v.id); return { alias: t ? t.name : v.id }; }
  if (v && typeof v === 'object' && 'r' in v) return hex(v);
  return v;
}

async function dump() {
  const out = { generated: new Date().toISOString(), file: figma.root.name, pages: [], collections: [], textStyles: [], components: {} };
  for (const c of await figma.variables.getLocalVariableCollectionsAsync()) {
    const col = { name: c.name, id: c.id, modes: c.modes.map(m => m.name), variables: [] };
    for (const id of c.variableIds) {
      const v = await figma.variables.getVariableByIdAsync(id);
      const values = {};
      for (const m of c.modes) values[m.name] = await value(v.valuesByMode[m.modeId]);
      col.variables.push({ name: v.name, type: v.resolvedType, description: v.description || '', scopes: v.scopes, values });
    }
    out.collections.push(col);
  }
  for (const s of await figma.getLocalTextStylesAsync()) {
    out.textStyles.push({ name: s.name, family: s.fontName.family, style: s.fontName.style, size: s.fontSize,
      lineHeight: s.lineHeight.unit === 'AUTO' ? 'auto' : s.lineHeight.value + (s.lineHeight.unit === 'PIXELS' ? 'px' : '%'),
      letterSpacing: s.letterSpacing.value + (s.letterSpacing.unit === 'PIXELS' ? 'px' : '%'), textCase: s.textCase, description: s.description || '' });
  }
  for (const page of figma.root.children) {
    await page.loadAsync();
    out.pages.push({ name: page.name, id: page.id, frames: page.children.filter(n => n.type !== 'TEXT').map(n => ({ id: n.id, name: n.name, w: Math.round(n.width), h: Math.round(n.height) })) });
    if (!/Components/.test(page.name)) continue;
    const list = [];
    for (const n of page.findAll(n => n.type === 'COMPONENT_SET' || (n.type === 'COMPONENT' && n.parent.type !== 'COMPONENT_SET'))) {
      let shelf = n.parent; while (shelf && shelf.parent && shelf.parent.type !== 'PAGE') shelf = shelf.parent;
      const defs = n.componentPropertyDefinitions || {};
      const props = Object.entries(defs).map(([k, d]) => ({ name: k.split('#')[0], type: d.type, default: d.defaultValue, options: d.variantOptions }));
      const size = n.type === 'COMPONENT_SET' ? n.children[0] : n;
      list.push({ name: n.name, id: n.id, kind: n.type === 'COMPONENT_SET' ? 'set' : 'component', variants: n.type === 'COMPONENT_SET' ? n.children.length : 1,
        w: Math.round(size.width), h: Math.round(size.height), shelf: shelf && shelf.type !== 'PAGE' ? shelf.name : '', description: dec(n.description), properties: props });
    }
    out.components[page.name] = list;
  }
  return out;
}

// Text outline of a frame: containers, texts, and instances (main component + property values; not descended).
async function outline(node, depth = 0, lines = []) {
  const pad = '  '.repeat(depth);
  const size = 'width' in node ? ` ${Math.round(node.width)}×${Math.round(node.height)}` : '';
  if (node.type === 'TEXT') { lines.push(`${pad}T "${node.characters.replace(/\n/g, '⏎')}"`); return lines; }
  if (node.type === 'INSTANCE') {
    const main = await node.getMainComponentAsync();
    const name = main ? (main.parent && main.parent.type === 'COMPONENT_SET' ? main.parent.name : main.name) : node.name;
    const props = Object.entries(node.componentProperties || {}).map(([k, p]) => `${k.split('#')[0]}=${p.value}`).join(', ');
    const texts = node.findAll(n => n.type === 'TEXT').map(t => t.characters.replace(/\n/g, '⏎')).filter(Boolean).slice(0, 12);
    lines.push(`${pad}I ${name}${props ? ' {' + props + '}' : ''}${size}${texts.length ? ' "' + texts.join('" "') + '"' : ''}`);
    return lines;
  }
  if (!('children' in node)) { if (depth < 2) lines.push(`${pad}${node.type[0]} ${node.name}${size}`); return lines; }
  const layout = node.layoutMode && node.layoutMode !== 'NONE' ? ` [${node.layoutMode[0]} gap ${node.itemSpacing}]` : '';
  lines.push(`${pad}F ${node.name}${size}${layout}`);
  for (const c of node.children) await outline(c, depth + 1, lines);
  return lines;
}

async function run(opts) {
  const items = ITEMS.filter(i => opts.groups.includes(i.group));
  const t0 = Date.now();
  let done = 0, failed = [];
  const original = figma.currentPage;
  for (const it of items) {
    status(`${++done}/${items.length}  ${it.png}`);
    try {
      const node = await figma.getNodeByIdAsync(it.id);
      if (!node) throw new Error('node not found');
      const png = await node.exportAsync({ format: 'PNG', constraint: { type: 'SCALE', value: opts.scale } });
      if (!(await post(it.png, png))) throw new Error('receiver rejected ' + it.png);
      if (it.svg && opts.svg) {
        const svg = await node.exportAsync({ format: 'SVG', svgOutlineText: true, svgIdAttribute: false });
        if (!(await post(it.svg, svg))) throw new Error('receiver rejected ' + it.svg);
      }
      if (opts.data && it.theme !== 'light') await postText(`docs/data/outline/${it.group}-${it.png.split('/').pop().replace('.png', '')}.txt`, (await outline(node)).join('\n'));
    } catch (e) { failed.push(it.id + ' ' + it.png + ': ' + e.message); }
  }
  if (opts.icons) {
    const sys = figma.root.children.find(p => p.name === 'v2 · System & Components');
    await sys.loadAsync();
    const comps = sys.findAll(n => n.type === 'COMPONENT' && /^NF2 (Icon|Cursor)\//.test(n.name));
    let k = 0;
    for (const c of comps) {
      status(`icons & cursors ${++k}/${comps.length}`);
      const path = c.name.startsWith('NF2 Icon/') ? 'exports/icons/' + c.name.slice(9) + '.svg' : 'exports/cursors/' + c.name.slice(11) + '.svg';
      try { await post(path, await c.exportAsync({ format: 'SVG', svgIdAttribute: false })); } catch (e) { failed.push(path + ': ' + e.message); }
    }
  }
  if (opts.data) {
    status('Writing docs/data/figma-dump.json …');
    try { await postText('docs/data/figma-dump.json', JSON.stringify(await dump(), null, 1)); } catch (e) { failed.push('dump: ' + e.message); }
  }
  await figma.setCurrentPageAsync(original);
  const report = { frames: items.length, failed, seconds: Math.round((Date.now() - t0) / 1000) };
  await postText('exports/export-report.json', JSON.stringify(report, null, 1));
  status(`Done: ${items.length} frames in ${report.seconds}s` + (failed.length ? ` — ${failed.length} failed (see exports/export-report.json)` : ''));
}

figma.ui.onmessage = m => {
  if (m.type === 'ack') { const r = pending[m.path]; delete pending[m.path]; if (r) r(m.ok); }
  if (m.type === 'start') run(m);
  if (m.type === 'close') figma.closePlugin();
};
