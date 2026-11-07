// Test-only native Cargo runner. A suite builds each tool once, then still runs the
// real executable for every fixture. Never cache between Node processes: Cargo must
// recheck the source at the start of each suite. Artifact paths come from Cargo,
// including when CARGO_TARGET_DIR moves the outputs away from target/debug.
const { spawnSync } = require('node:child_process');
const { existsSync, readFileSync, realpathSync } = require('node:fs');
const { homedir } = require('node:os');
const { resolve, dirname } = require('node:path');

const executables = new Map();

function nativeConfig(cwd, env) {
  const directories = new Set();
  // Cargo searches cwd and its ancestors, plus CARGO_HOME. Check both lexical and
  // physical ancestors conservatively when the caller's cwd contains a symlink.
  let physical;
  try { physical = realpathSync(cwd); } catch { return null; }
  for (const start of [cwd, physical]) {
    let directory = start;
    while (true) {
      directories.add(resolve(directory, '.cargo'));
      const parent = dirname(directory);
      if (parent === directory) break;
      directory = parent;
    }
  }
  const home = (process.platform === 'win32' ? env.USERPROFILE : env.HOME) || homedir();
  directories.add(env.CARGO_HOME ? resolve(cwd, env.CARGO_HOME) : resolve(home, '.cargo'));
  const files = [];
  let deploymentTarget;
  for (const directory of directories) {
    for (const name of ['config', 'config.toml']) {
      const path = resolve(directory, name);
      let source;
      try { source = readFileSync(path, 'utf8'); } catch (error) {
        if (error.code === 'ENOENT') continue;
        return null;
      }
      const text = source.replace(/\r\n/g, '\n').trim();
      // This is an exact whitelist, not a partial TOML parser. Anything beyond
      // the repository's one env setting (including multiline target/rustflags,
      // includes and custom runners) stays with Cargo's own run semantics.
      if (text === '[env]\nMACOSX_DEPLOYMENT_TARGET = "15.0"') deploymentTarget = '15.0';
      else if (text !== '') return null;
      files.push([path, source]);
    }
  }
  // Cargo's [env] also applies to the executed program; the default is force=false.
  return { files, env: deploymentTarget && env.MACOSX_DEPLOYMENT_TARGET === undefined
    ? { ...env, MACOSX_DEPLOYMENT_TARGET: deploymentTarget } : env };
}

function nativeTool(args) {
  if (args[0] !== 'run') return null;
  const separator = args.indexOf('--');
  const buildArgs = args.slice(1, separator < 0 ? args.length : separator);
  let name, example;
  for (let i = 0; i < buildArgs.length; i++) {
    const arg = buildArgs[i];
    if (arg === '-p' || arg === '--package') name = buildArgs[++i];
    else if (arg === '--example') example = buildArgs[++i];
    else if (arg !== '--quiet' && arg !== '--locked') return null;
  }
  // Only these native tools are used by the workflow suite. Other Cargo commands
  // retain Cargo's own target/runner/profile semantics instead of guessing them.
  if (!(name === 'neoflash-export' && !example)
      && !(name === 'neoflash-script' && ['compile_batch', 'compile_project'].includes(example))) return null;
  if (!buildArgs.includes('--locked')) buildArgs.push('--locked');
  return { buildArgs, name: example || name, kind: example ? 'example' : 'bin',
    programArgs: separator < 0 ? [] : args.slice(separator + 1) };
}

function runCargo(cargo, args, options = {}) {
  const tool = nativeTool(args);
  const env = options.env || process.env;
  const cwd = resolve(options.cwd || process.cwd());
  const config = nativeConfig(cwd, env);
  // Cross targets, custom runners and dynamic Rust linking need Cargo's runtime
  // setup. The ordinary suite uses native, statically linked Rust tools.
  if (!tool || !config || env.CARGO_BUILD_TARGET || Object.keys(env).some(key => /^CARGO_TARGET_.*_RUNNER$/.test(key))
      || Object.entries(env).some(([key, value]) => /RUSTFLAGS$/.test(key) && /prefer-dynamic/.test(value || ''))
      || options.shell || options.stdio || options.encoding !== 'utf8') {
    // A fallback Cargo run can overwrite an artifact already cached for another
    // configuration. Never reuse that path when the caller switches back.
    executables.clear();
    return spawnSync(cargo, args, options);
  }
  const runOptions = { ...options, env: config.env };
  const key = JSON.stringify([cargo, cwd, tool.buildArgs, config.files,
    Object.entries(config.env).filter(([, value]) => value !== undefined).sort(([a], [b]) => a.localeCompare(b))]);
  let executable = executables.get(key);
  let diagnostics = '';
  if (!executable || !existsSync(executable)) {
    const build = spawnSync(cargo, ['build', ...tool.buildArgs, '--message-format=json'], {
      ...runOptions, input: undefined, encoding: 'utf8', maxBuffer: Math.max(options.maxBuffer || 0, 16 * 1024 * 1024),
    });
    diagnostics = build.stderr || '';
    for (const line of (build.stdout || '').split('\n')) {
      let message;
      try { message = JSON.parse(line); } catch { continue; }
      if (message.reason === 'compiler-message') diagnostics += message.message?.rendered || '';
      if (message.reason === 'compiler-artifact' && message.target?.name === tool.name
          && message.target.kind?.includes(tool.kind) && message.executable) executable = message.executable;
    }
    if (build.status !== 0 || !executable || !existsSync(executable)) {
      const stderr = diagnostics || `Cargo did not produce the ${tool.name} executable.\n`;
      return { ...build, status: build.status === 0 ? 1 : build.status,
        stdout: '', stderr, output: [null, '', stderr] };
    }
    // Different Rust flags can overwrite the same artifact path. Returning to an
    // earlier environment must rebuild, not reuse the newer environment's file.
    for (const [otherKey, path] of executables) {
      if (path === executable) executables.delete(otherKey);
    }
    executables.set(key, executable);
  }
  const result = spawnSync(executable, tool.programArgs, runOptions);
  if (diagnostics) {
    result.stderr = diagnostics + (result.stderr || '');
    result.output = [result.output?.[0] ?? null, result.stdout, result.stderr];
  }
  return result;
}

module.exports = { runCargo };
