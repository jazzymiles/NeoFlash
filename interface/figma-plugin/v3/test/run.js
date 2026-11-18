#!/usr/bin/env node
'use strict';
// Runs the NF3 Figma builder (src/*.js) against test/mock-figma.js.
//
//   node test/run.js [--job "<statements>"] [extra-file.js ...]
//   node test/run.js --list-icons
//   node test/run.js --list-atoms
//
// The sources are concatenated in file-name order and evaluated in one function body, like the plugin bridge does:
//   new Function('figma', src + '\nreturn (async () => {' + JOB + '\n})();')(figma)
// (plus one harness statement before "return" that hands NF to this script). JOB = the atoms prelude followed
// by the --job statements. The returned value goes to stdout as JSON; warnings (unique NF.warn entries) and
// errors go to stderr, errors with the stack mapped back to <file>:<line>. Exit code 1 on any thrown error,
// 2 on a usage error. Icons come from interface/design-system/exports/icons (override: NF3_ICONS_DIR).
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const { createFigma } = require('./mock-figma.js');

const ROOT = path.resolve(__dirname, '..');
const SRC = path.join(ROOT, 'src');
const ICONS = process.env.NF3_ICONS_DIR || path.resolve(ROOT, '..', '..', 'design-system', 'exports', 'icons');
const DENSITY = path.join(SRC, '05-density.js');
const PRELUDE = 'await nfInit(); await figma.setCurrentPageAsync(NF.atomsPage); nf3AtomsA(); nf3AtomsB(); await nfInit();';
const DEFAULT_JOB = 'await nf3Concept(); await nfFlush(); return nfReport({});';
const BUNDLE = 'nf3-bundle.js';
const USAGE = 'usage: node test/run.js [--job "<statements>"] [--list-icons] [--list-atoms] [extra-file.js ...]';

function usage(msg) { process.stderr.write((msg ? msg + '\n' : '') + USAGE + '\n'); process.exit(msg ? 2 : 0); }

function parseArgs(argv) {
  const o = { job: null, listIcons: false, listAtoms: false, files: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--job') { if (i + 1 >= argv.length) usage('--job needs a value'); o.job = argv[++i]; }
    else if (a.indexOf('--job=') === 0) o.job = a.slice(6);
    else if (a === '--list-icons') o.listIcons = true;
    else if (a === '--list-atoms') o.listAtoms = true;
    else if (a === '-h' || a === '--help') usage();
    else if (a[0] === '-') usage('unknown option ' + a);
    else o.files.push(a);
  }
  return o;
}

// src/*.js in name order, then the extra files in name order; a file already in src is not loaded twice.
function collectFiles(extra) {
  const src = fs.readdirSync(SRC).filter(f => f.endsWith('.js')).sort().map(f => path.join(SRC, f));
  const seen = new Set(src.map(f => fs.realpathSync(f)));
  const more = [];
  for (const f of extra) {
    const p = [path.resolve(f), path.resolve(ROOT, f), path.resolve(SRC, f)].find(x => fs.existsSync(x));
    if (!p) usage('file not found: ' + f);
    const real = fs.realpathSync(p);
    if (!seen.has(real)) { seen.add(real); more.push(real); }
  }
  more.sort((a, b) => (path.basename(a) < path.basename(b) ? -1 : path.basename(a) > path.basename(b) ? 1 : 0));
  return src.concat(more);
}
const label = file => { const rel = path.relative(ROOT, file); return rel.indexOf('..') === 0 ? file : rel; };

// A syntax error inside new Function() has no usable position, so each part is compiled on its own first.
function syntaxCheck(code, name, wrap) {
  try { new vm.Script(wrap[0] + code + wrap[1], { filename: name }); return null; }
  catch (e) {
    if (!(e instanceof SyntaxError)) throw e;
    const m = String(e.stack).match(/^(.*):(\d+)\n/);
    return { message: e.message, line: m ? Number(m[2]) : 0 };
  }
}

function build(files, userJob) {
  const parts = files.map(f => ({ file: f, name: label(f), code: fs.readFileSync(f, 'utf8') }));
  let line = 1;
  for (const p of parts) {
    p.start = line; p.lines = p.code.split('\n').length; line += p.lines;
    const bad = syntaxCheck(p.code, p.name, ['(function () {', '\n})']);
    if (bad) throw Object.assign(new Error('SyntaxError: ' + bad.message), { nfWhere: p.name + (bad.line ? ':' + Math.min(bad.line, p.lines) : '') });
  }
  const badJob = syntaxCheck(userJob, '<job>', ['(async () => {' + PRELUDE + '\n', '\n})']);
  if (badJob) throw Object.assign(new Error('SyntaxError: ' + badJob.message), { nfWhere: '<job>:' + Math.min(Math.max(badJob.line - 1, 1), userJob.split('\n').length) });
  const jobLine = line; // the line of "return (async () => {" + prelude; the --job statements start on the next line
  const body = parts.map(p => p.code).join('\n') +
    '\n__harness.NF = typeof NF === "undefined" ? null : NF; return (async () => {' + PRELUDE + '\n' + userJob + '\n})();' +
    '\n//# sourceURL=' + BUNDLE;
  // Line of the first body line inside the function that new Function() generates (normally 3).
  const probe = String(new Function('figma', '__harness', 'return new Error("probe").stack;\n//# sourceURL=nf3-probe.js')());
  const pm = probe.match(/(?:nf3-probe\.js|<anonymous>):(\d+):\d+/);
  const offset = pm ? Number(pm[1]) - 1 : 2;
  function locate(stackLine) {
    const n = stackLine - offset;
    if (n === jobLine) return '<job prelude>';
    if (n > jobLine) return '<job>:' + (n - jobLine);
    for (const p of parts) if (n >= p.start && n < p.start + p.lines) return p.name + ':' + (n - p.start + 1);
    return '<bundle>:' + n;
  }
  return { body: body, locate: locate };
}

function mapStack(err, locate) {
  const out = [];
  for (const l of String(err && err.stack || '').split('\n')) {
    const m = l.match(/^\s*at (?:async )?(?:(.*?) \()?.*?(?:nf3-bundle\.js|<anonymous>):(\d+):\d+\)?$/);
    if (!m) continue;
    const fn = m[1] && m[1] !== 'eval' && m[1].indexOf('eval at') !== 0 ? m[1].replace(/^Object\./, '') : '';
    out.push({ where: locate(Number(m[2])), fn: fn });
  }
  return out;
}

function printWarnings(harness) {
  const NF = harness.NF;
  if (!NF) return;
  const warn = Array.from(new Set(NF.warn || []));
  if (!warn.length) process.stderr.write('warnings: none\n');
  else process.stderr.write('warnings (' + warn.length + '):\n' + warn.map(w => '  - ' + w).join('\n') + '\n');
}

async function run(figma, files, userJob) {
  const harness = { NF: null };
  let bundle;
  try { bundle = build(files, userJob); }
  catch (e) { if (!e.nfWhere) throw e; process.stderr.write('ERROR: ' + e.message + '\n  at ' + e.nfWhere + '\n'); process.exit(1); }
  try {
    const result = await new Function('figma', '__harness', bundle.body)(figma, harness);
    return { result: result, harness: harness };
  } catch (e) {
    const frames = mapStack(e, bundle.locate);
    const name = e && e.name && e.name !== 'Error' ? e.name + ': ' : '';
    process.stderr.write('ERROR: ' + name + (e && e.message !== undefined ? e.message : String(e)) + '\n');
    if (frames.length) process.stderr.write(frames.slice(0, 8).map(f => '  at ' + f.where + (f.fn ? ' (' + f.fn + ')' : '')).join('\n') + '\n');
    else process.stderr.write('  (no source location inside the bundle)\n' + String(e && e.stack || '').split('\n').slice(1, 6).join('\n') + '\n');
    printWarnings(harness);
    process.exit(1);
  }
}

async function listAtoms(figma, files) {
  const { harness } = await run(figma, files, 'return null;');
  const atoms = harness.NF.atoms, lines = [];
  for (const name of Object.keys(atoms).sort()) {
    const node = atoms[name], defs = node.componentPropertyDefinitions;
    const isSet = node.type === 'COMPONENT_SET';
    lines.push(node.name + '  [' + (isSet ? node.children.length + ' variants' : 'component') + ']');
    for (const key of Object.keys(defs)) {
      const d = defs[key];
      if (d.type === 'VARIANT') { lines.push('  variant  ' + key + ': ' + d.variantOptions.join(' | ')); continue; }
      let def = JSON.stringify(d.defaultValue);
      if (d.type === 'INSTANCE_SWAP') { const c = await figma.getNodeByIdAsync(d.defaultValue); def = c ? c.name : def; }
      lines.push('  prop     ' + key.split('#')[0] + ': ' + d.type + ' = ' + def);
    }
  }
  process.stdout.write(lines.join('\n') + '\n');
  printWarnings(harness);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!fs.existsSync(SRC)) usage('missing ' + SRC);
  const figma = createFigma({ iconsDir: ICONS, densityFile: DENSITY });
  if (!figma.__mock.iconCount) process.stderr.write('note: no icon SVGs found under ' + ICONS + ' (every icon will be reported as missing)\n');
  if (args.listIcons) {
    const names = figma.root.findAllWithCriteria({ types: ['COMPONENT'] }).map(c => c.name)
      .filter(n => n.indexOf('NF2 Icon/16/') === 0).map(n => n.slice(12));
    process.stdout.write(names.join('\n') + '\n');
    return;
  }
  const files = collectFiles(args.files);
  if (args.listAtoms) return listAtoms(figma, files);
  const { result, harness } = await run(figma, files, args.job == null ? DEFAULT_JOB : args.job);
  process.stdout.write((result === undefined ? 'null' : JSON.stringify(result, null, 2)) + '\n');
  printWarnings(harness);
}

main().catch(e => { process.stderr.write('ERROR: ' + (e && e.stack || e) + '\n'); process.exit(1); });
