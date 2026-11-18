// NeoFlash Export — sends PNG/SVG exports and a JSON dump of the design system
// to the local receiver (tools/receiver.py), which writes them into design-system/.
// ITEMS is injected by build.py from exports/manifest.json.
const ITEMS = /*ITEMS*/[];

figma.showUI(__html__, { width: 400, height: 330 });

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
    if (!/Components|^v3 · Atoms$/.test(page.name)) continue;
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

// Boards marked `split` hold several panels / dialogs; each one is also exported on its own,
// named after its layer (“Panel / Properties — Brush tool” → panel-properties-brush-tool).
const PART = /^(Panel|Dialog|Window|Sample|Menu|Flyout|Canvas) [\/—]/;
const slugify = s => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
function parts(node, out = []) {
  for (const c of node.children || []) {
    if (!c.visible) continue;
    if (PART.test(c.name) && (c.type === 'FRAME' || c.type === 'INSTANCE')) out.push(c);
    else if (c.type === 'FRAME' || c.type === 'GROUP') parts(c, out);
  }
  return out;
}
async function exportParts(node, it, opts, used, failed) {
  let n = 0;
  for (const p of parts(node)) {
    let slug = slugify(p.name); const k = it.theme + '/' + slug;
    used[k] = (used[k] || 0) + 1; if (used[k] > 1) slug += '-' + used[k];
    const base = `exports/panels/${it.theme}`;
    try {
      await post(`${base}/png/${slug}.png`, await p.exportAsync({ format: 'PNG', constraint: { type: 'SCALE', value: opts.scale } }));
      if (opts.svg) await post(`${base}/svg/${slug}.svg`, await p.exportAsync({ format: 'SVG', svgOutlineText: true, svgIdAttribute: false }));
      n++;
    } catch (e) { failed.push(p.id + ' ' + slug + ': ' + e.message); }
  }
  return n;
}

// v3 reference pages are found by name ("v3 · <Density> · <Theme>"), so a rebuild of the Figma pages needs no
// manifest edit. Every board (a top-level frame whose first child is "Board title") becomes one file.
const V3_DENSITIES = [['Normal', 'normal'], ['Compact', 'compact'], ['Classic Flash', 'classic-flash']];
const V3_THEMES = [['Light', 'light'], ['Dark', 'dark'], ['Mid Neutral Gray', 'mid-neutral-gray'], ['Dark Graphite', 'dark-graphite']];
const V3_OUTLINE_PAGE = 'v3 · Normal · Light';
const v3Boards = page => page.children.filter(n => n.type === 'FRAME' && n.children.length && n.children[0].name === 'Board title');
async function v3Frame(node, base, slug, opts, failed) {
  try {
    if (!(await post(`${base}/png/${slug}.png`, await node.exportAsync({ format: 'PNG', constraint: { type: 'SCALE', value: opts.scale } })))) throw new Error('receiver rejected the PNG');
    if (opts.svg && !(await post(`${base}/svg/${slug}.svg`, await node.exportAsync({ format: 'SVG', svgOutlineText: true, svgIdAttribute: false })))) throw new Error('receiver rejected the SVG');
    return 1;
  } catch (e) { failed.push(`${base}/${slug}: ${e.message}`); return 0; }
}
async function runV3(opts, failed) {
  const pageNamed = name => figma.root.children.find(p => p.name === name);
  const combos = [];
  for (const d of V3_DENSITIES) for (const t of V3_THEMES) if (opts.v3.densities.includes(d[1]) && opts.v3.themes.includes(t[1])) combos.push([d, t]);
  let frames = 0, pages = 0;
  for (const [d, t] of combos) {
    const name = `v3 · ${d[0]} · ${t[0]}`, page = pageNamed(name);
    if (!page) { failed.push(name + ': page not found'); continue; }
    await page.loadAsync(); pages++;
    const boards = v3Boards(page), used = {};
    let k = 0;
    for (const b of boards) {
      status(`v3 ${pages}/${combos.length}  ${d[1]}/${t[1]}  ${++k}/${boards.length}  ${b.name}`);
      let slug = slugify(b.name); used[slug] = (used[slug] || 0) + 1; if (used[slug] > 1) slug += '-' + used[slug];
      frames += await v3Frame(b, `exports/v3/${d[1]}/${t[1]}`, slug, opts, failed);
    }
  }
  if (opts.v3.system) {
    const concept = pageNamed('v3 · Concept'), atoms = pageNamed('v3 · Atoms');
    if (concept) { await concept.loadAsync(); for (const n of concept.children) if (n.type === 'FRAME') { status('v3 system  ' + n.name); frames += await v3Frame(n, 'exports/v3/system', 'concept-' + slugify(n.name), opts, failed); } }
    else failed.push('v3 · Concept: page not found');
    if (atoms) { await atoms.loadAsync(); for (const n of atoms.children) if (n.type === 'FRAME' && n.name.indexOf('NF3 /') === 0) { status('v3 system  ' + n.name); frames += await v3Frame(n, 'exports/v3/system', slugify(n.name), opts, failed); } }
    else failed.push('v3 · Atoms: page not found');
  }
  if (opts.v3.data) {
    // Structure is the same in every theme and density; the sizes in the outlines are the Normal ones.
    const page = pageNamed(V3_OUTLINE_PAGE);
    if (!page) failed.push(V3_OUTLINE_PAGE + ': page not found');
    else {
      await page.loadAsync();
      const boards = v3Boards(page), used = {};
      let k = 0;
      for (const b of boards) {
        status(`v3 outlines ${++k}/${boards.length}  ${b.name}`);
        let slug = slugify(b.name); used[slug] = (used[slug] || 0) + 1; if (used[slug] > 1) slug += '-' + used[slug];
        try { await postText(`docs/data/outline-v3/${slug}.txt`, (await outline(b)).join('\n')); } catch (e) { failed.push('outline ' + slug + ': ' + e.message); }
      }
    }
    // A separate file: crates/neoflash-ui/build.rs reads docs/data/figma-dump.json and must keep its v2.25 input.
    status('Writing docs/data/figma-dump-v3.json …');
    try { await postText('docs/data/figma-dump-v3.json', JSON.stringify(await dump(), null, 1)); } catch (e) { failed.push('v3 dump: ' + e.message); }
  }
  return { pages, frames };
}

async function run(opts) {
  const items = ITEMS.filter(i => opts.groups.includes(i.group));
  const t0 = Date.now();
  let done = 0, failed = [], singles = 0;
  const used = {};
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
      // Outlines describe structure, which is shared by every palette. Export
      // document frames and the canonical Dark theme so later palette clones
      // cannot overwrite the same outline file.
      if (opts.data && (it.group === 'docs' || it.theme === 'dark')) await postText(`docs/data/outline/${it.group}-${it.png.split('/').pop().replace('.png', '')}.txt`, (await outline(node)).join('\n'));
      if (it.split) singles += await exportParts(node, it, opts, used, failed);
    } catch (e) { failed.push(it.id + ' ' + it.png + ': ' + e.message); }
  }
  const v3 = opts.v3 && opts.v3.on ? await runV3(opts, failed) : null;
  if (opts.icons) {
    // Prefer the current live component page while retaining older export compatibility.
    // Icon masters live on "v3 · Atoms" since v3; cursors stayed on the (now archived) v2.25 component page.
    const comps = [];
    for (const page of figma.root.children) {
      if (page.name !== 'v3 · Atoms' && !/^v2(\.\d+)? · System & Components/.test(page.name)) continue;
      await page.loadAsync();
      for (const n of page.findAll(n => n.type === 'COMPONENT' && /^NF2 (Icon|Cursor)\//.test(n.name))) if (!comps.some(c => c.name === n.name)) comps.push(n);
    }
    if (!comps.length) throw new Error('No NF2 Icon / Cursor components found');
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
  const report = { frames: items.length, singlePanels: singles, v3, failed, seconds: Math.round((Date.now() - t0) / 1000) };
  await postText('exports/export-report.json', JSON.stringify(report, null, 1));
  status('Done: ' + (v3 ? `${v3.frames} v3 frames from ${v3.pages} pages, ` : '') + `${items.length} v2.25 frames + ${singles} single panels in ${report.seconds}s` + (failed.length ? ` — ${failed.length} failed (see exports/export-report.json)` : ''));
}

figma.ui.onmessage = m => {
  if (m.type === 'ack') { const r = pending[m.path]; delete pending[m.path]; if (r) r(m.ok); }
  if (m.type === 'start') run(m);
  if (m.type === 'close') figma.closePlugin();
};
