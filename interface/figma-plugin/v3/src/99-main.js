// NF3 builder — entry point when the builder runs as a Figma development plugin (manifest.json next to
// build.sh). It rebuilds everything: density variables, atoms, concept, the master reference page and its
// eleven copies. This file is NOT stored on the "v3 · Builder source" page (the MCP runner sends jobs instead).
async function nf3Main() {
  const say = m => { try { figma.notify(m, { timeout: 1500 }); } catch (e) { /* notify is optional */ } };
  say('NF3: density variables'); await nf3BuildDensity();
  await nfInit(); await nf3Setup(); await nfInit();
  if (typeof nf3MoreVars === 'function') nf3MoreVars();
  say('NF3: atoms'); await figma.setCurrentPageAsync(NF.atomsPage);
  nf3AtomsA(); nf3AtomsB(); await nfFlush(); await nfInit();
  await nf3AtomsMatrix(); await nfFlush();
  say('NF3: concept'); await nf3Concept(); await nfFlush();
  const builds = [
    typeof nf3BuildB === 'function' ? nf3BuildB : null, typeof nf3BuildC === 'function' ? nf3BuildC : null,
    typeof nf3BuildD === 'function' ? nf3BuildD : null,
    typeof nf3BuildPropsA === 'function' ? nf3BuildPropsA : null, typeof nf3BuildPropsB === 'function' ? nf3BuildPropsB : null,
    typeof nf3BuildPanelsA === 'function' ? nf3BuildPanelsA : null, typeof nf3BuildPanelsB === 'function' ? nf3BuildPanelsB : null,
    typeof nf3BuildPanelsC === 'function' ? nf3BuildPanelsC : null,
    typeof nf3BuildDialogsA === 'function' ? nf3BuildDialogsA : null, typeof nf3BuildDialogsB === 'function' ? nf3BuildDialogsB : null,
    typeof nf3BuildDialogsC === 'function' ? nf3BuildDialogsC : null, typeof nf3BuildDialogsD === 'function' ? nf3BuildDialogsD : null,
    typeof nf3BuildDialogsE === 'function' ? nf3BuildDialogsE : null, typeof nf3BuildDialogsF === 'function' ? nf3BuildDialogsF : null,
    typeof nf3BuildMenusA === 'function' ? nf3BuildMenusA : null, typeof nf3BuildMenusB === 'function' ? nf3BuildMenusB : null,
    typeof nf3BuildScreensA === 'function' ? nf3BuildScreensA : null, typeof nf3BuildScreensB === 'function' ? nf3BuildScreensB : null,
  ].filter(Boolean);
  let n = 0;
  for (const b of builds) { n++; say('NF3: boards ' + n + ' / ' + builds.length); await b(); await nfFlush(); }
  for (const d of NF_DENSITIES) for (const t of NF_THEMES) {
    say('NF3: page ' + d + ' · ' + t[1]);
    const r = await nf3ClonePage(d, t[0]);
    if (r && d !== 'Normal') { const p = figma.root.children.find(x => x.name === r.name); await figma.setCurrentPageAsync(p); nfArrange(p); }
  }
  nf3OrderPages();
  return Array.from(new Set(NF.warn));
}
nf3Main().then(w => figma.closePlugin(w.length ? 'NF3 built with warnings: ' + w.slice(0, 3).join('; ') : 'NF3 built'))
  .catch(e => figma.closePlugin('NF3 failed: ' + (e && e.message ? e.message : e)));
