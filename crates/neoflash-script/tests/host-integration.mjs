// Execute the real host proxy/dispatch implementation with a small Player stub.
// Console, host trace and on-page feedback have independent observations so a
// compiler that bypasses host trace cannot accidentally pass this regression.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { runtimeWasm } from './rust-runtime-host.mjs';
const require = createRequire(import.meta.url);

export function createRuntimeHost(root, player, compiledScripts, options = {}) {
  const path = resolve(root, 'web/runtime.js');
  const runtime = readFileSync(path, 'utf8');
  const runtimeExports = require(path);
  const asIntrinsicsPath = resolve(root, 'web/as-intrinsics.js');
  try {
    require(asIntrinsicsPath);
  } catch (_) {}
  if (options.bundle && typeof options.bundle === 'object' && !options.bundle.as_prelude && typeof globalThis.__neoflashAsIntrinsics === 'string') {
    Object.defineProperty(options.bundle, 'as_prelude', {
      configurable: true,
      enumerable: false,
      writable: true,
      value: globalThis.__neoflashAsIntrinsics,
    });
  }
  const { completeCleanup, createScenePainter, sourceKey, oneBasedFrame, as2Frame, createAs2Methods, describeThrown, unwrapScriptThrow, scriptFunction, functionBodyLineOffset, mappedErrorFrames, sourceLocationText, formatTraceValue, formatTraceArgs, emitScriptMessage } = runtimeExports;
  const hydrateAsPrelude = typeof runtimeExports.hydrateAsPrelude === 'function'
    ? runtimeExports.hydrateAsPrelude
    : function hydrateAsPreludeFallback(code, prelude) {
        if (typeof code !== 'string' || typeof prelude !== 'string' || !prelude) return code;
        const marker = '/*__NF_AS_PRELUDE__*/';
        const idx = code.indexOf(marker);
        if (idx < 0) return code;
        let nlCount = 0;
        for (let i = 0; i < prelude.length; i++) {
          if (prelude.charCodeAt(i) === 10) nlCount++;
        }
        let end = idx + marker.length;
        while (nlCount > 0 && code.charCodeAt(end) === 10) {
          end++;
          nlCount--;
        }
        return code.slice(0, idx) + prelude + code.slice(end);
      };
  const compileProgramUnit = typeof runtimeExports.compileProgramUnit === 'function'
    ? runtimeExports.compileProgramUnit
    : function compileProgramUnitFallback(programUnit, prelude) {
        const resolvedCode = hydrateAsPrelude(programUnit.code, prelude);
        const url = programUnit?.source_url || 'neoflash-project.js';
        return new Function('__nfBuiltins', resolvedCode + '\n//# sourceURL=' + url);
      };
  const start = runtime.indexOf('    const propertyMap =');
  const end = runtime.indexOf('    function resize()', start);
  assert.ok(start >= 0 && end > start, 'host implementation boundaries changed');
  const scriptConsole = [], hostConsole = [], traceCalls = [], scriptMessages = [];
  if(options.context&&!options.context.save){const stack=[];options.context.save=function(){stack.push({font:this.font,fontKerning:this.fontKerning,letterSpacing:this.letterSpacing});};options.context.restore=function(){Object.assign(this,stack.pop());};}
  const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame', '__nfAs2Method', '__nfAs2Value', '__nfProgram', '__nfDisplay', '__nfLegacy', '__nfXML'];
  const sourceUnits = compiledScripts.map((entry, index) => {
    const published = options.bundle?.scripts?.find(unit => sourceKey(unit) === sourceKey(entry.script));
    return {...published, ...entry, source_url: entry.source_url || published?.source_url || `neoflash-script-${index}.js`};
  });
  const bodyOffset = functionBodyLineOffset();
  const mappedUnits = sourceUnits.concat(options.bundle?.program ? [options.bundle.program] : []);
  const sourceFrames = error => mappedErrorFrames(error, mappedUnits, bodyOffset);
  const functions = options.functions instanceof Map ? options.functions : new Map();
  for (const unit of sourceUnits) {
    const hostGlobal=options.external?.hostGlobal||globalThis;
    const compiled = scriptFunction(unit, [...bindings, 'console', 'globalThis', 'window', 'document'], options.bundle?.as_prelude);
    const fn = function(...args) { return compiled.call(this, ...args,
      {log: (...values) => scriptConsole.push(values)}, hostGlobal, hostGlobal, hostGlobal.document); };
    functions.set(sourceKey(unit.script), { fn, location: unit.location });
  }
  const globals = { NeoFlashDisplay:require(resolve(root,'web/display.js')), NeoFlashEvents:require(resolve(root,'web/events.js')),
    NeoFlashStage:require(resolve(root,'web/stage.js')), NeoFlashLegacyInput:require(resolve(root,'web/legacy-input.js')), NeoFlashAccessibility:require(resolve(root,'web/accessibility.js')) };
  globals.NeoFlashAS2DepthManager=require(resolve(root,'web/as2-depth-manager.js'));
  globals.NeoFlashAS2Transitions=require(resolve(root,'web/as2-transitions.js'));
  const as2PrintModule=require(resolve(root,'web/as2-print.js'));
  globals.NeoFlashAS2Print={...as2PrintModule,create:(wasm,settings)=>as2PrintModule.create(wasm,{...settings,...options.print})};
  globals.NeoFlashAS2Boxing=require(resolve(root,'web/as2-boxing.js'));
  const as2CaptureModule=require(resolve(root,'web/as2-capture.js'));
  globals.NeoFlashAS2Capture={...as2CaptureModule,create:(wasm,settings)=>as2CaptureModule.create(wasm,{...settings,...options.capture})};
  const as2ConnectionsModule=require(resolve(root,'web/as2-connections.js'));
  globals.NeoFlashAS2Connections={...as2ConnectionsModule,create:settings=>as2ConnectionsModule.create({...settings,...options.connections})};
  const as2StageModule=require(resolve(root,'web/as2-stage.js'));
  globals.NeoFlashAS2Stage={...as2StageModule,create:settings=>as2StageModule.create({...settings,...options.as2Stage})};
  globals.NeoFlashAS2Media=require(resolve(root,'web/as2-media.js'));
  globals.NeoFlashAS2Services=require(resolve(root,'web/as2-services.js'));
  const as2FilesModule=require(resolve(root,'web/as2-files.js'));
  globals.NeoFlashAS2Files={...as2FilesModule,create:settings=>as2FilesModule.create({...settings,...options.files})};
  globals.NeoFlashAS2Locale=require(resolve(root,'web/as2-locale.js'));
  globals.NeoFlashAS2Network=require(resolve(root,'web/as2-network.js'));
  globals.NeoFlashAS2Object=require(resolve(root,'web/as2-object.js'));
  globals.NeoFlashAS2Text=require(resolve(root,'web/as2-text.js'));
  globals.NeoFlashAS2Binding=require(resolve(root,'web/as2-binding.js'));
  globals.NeoFlashAS2Data=require(resolve(root,'web/as2-data.js'));
  const as2PlatformModule=require(resolve(root,'web/as2-platform.js'));
  globals.NeoFlashAS2Platform={...as2PlatformModule,create:(wasm,settings)=>as2PlatformModule.create(wasm,{...settings,...options.platform})};
  globals.NeoFlashAS2Strings=require(resolve(root,'web/as2-strings.js'));
  globals.NeoFlashAS3Strings=require(resolve(root,'web/as3-strings.js'));
  globals.NeoFlashDate=require(resolve(root,'web/date.js'));
  globals.NeoFlashNumberFormat=require(resolve(root,'web/number-format.js'));
  globals.NeoFlashGeom3D=require(resolve(root,'web/geom3d.js'));
  globals.NeoFlashEasing=require(resolve(root,'web/easing.js'));
  globals.NeoFlashBroadcaster=require(resolve(root,'web/broadcaster.js'));
  globals.NeoFlashMath=require(resolve(root,'web/math.js'));
  globals.NeoFlashWasm=runtimeWasm;
  const stageModule=globals.NeoFlashStage;
  globals.NeoFlashStage={...stageModule,create:settings=>stageModule.create({...settings,...options.stage})};
  globals.NeoFlashByteArray=require(resolve(root,'web/byte-array.js'));
  globals.NeoFlashPaths=require(resolve(root,'web/path.js'));
  const netModule=require(resolve(root,'web/net.js'));
  globals.NeoFlashNet={create:settings=>netModule.create({...settings,...options.net})};
  const bitmapModule=require(resolve(root,'web/bitmap.js'));
  globals.NeoFlashBitmap={...bitmapModule,create:settings=>bitmapModule.create({...settings,...options.bitmap})};
  if(typeof player.filter_rgba==='function'){
    const filtersModule=require(resolve(root,'web/filters.js'));
    globals.NeoFlashFilters={create:settings=>filtersModule.create({...settings,...options.filters})};
  }
  const textModule=require(resolve(root,'web/text.js'));
  globals.NeoFlashText={...textModule,create:settings=>textModule.create({...settings,...options.text})};
  if(typeof player.object_text_input==='function'){
    const textInputModule=require(resolve(root,'web/text-input.js'));
    globals.NeoFlashTextInput={create:settings=>textInputModule.create({...settings,...options.textInput})};
  }
  const audioModule=require(resolve(root,'web/audio.js'));
  globals.NeoFlashAudio={create:settings=>audioModule.create({...settings,...options.audio})};
  if(typeof player.object_video_info==='function'){
    const videoModule=require(resolve(root,'web/video.js'));
    globals.NeoFlashVideo={create:settings=>videoModule.create({...settings,...options.video})};
  }
  const xmlModule=require(resolve(root,'web/xml.js'));
  globals.NeoFlashXML={create:settings=>xmlModule.create({...settings,...options.xml})};
  const loaderModule=require(resolve(root,'web/loader.js'));
  if(typeof player.pixel_decode==='function')globals.NeoFlashLoader={create:settings=>loaderModule.create({...settings,...options.loader})};
  if(typeof player.amf_encode==='function'){
    const amfModule=require(resolve(root,'web/amf.js'));
    globals.NeoFlashAMF={...amfModule,create:settings=>amfModule.create({...settings,...options.amf})};
    const storageModule=require(resolve(root,'web/storage.js'));
    globals.NeoFlashStorage={create:settings=>storageModule.create({...settings,...options.storage})};
  }
  globals.NeoFlashGeom=require(resolve(root,'web/geom.js'));
  const externalModule=require(resolve(root,'web/external.js'));
  globals.NeoFlashExternal={create:settings=>externalModule.create({...settings,...options.external})};
  const airModule=require(resolve(root,'web/air.js'));
  globals.NeoFlashAIR={...airModule,create:settings=>airModule.create({...settings,...options.air})};
  const as3NetSysModule=require(resolve(root,'web/as3-net-sys.js'));
  globals.NeoFlashAS3NetSys={...as3NetSysModule,create:settings=>as3NetSysModule.create({...settings,...options.netSys})};
  const fte3dModule=require(resolve(root,'web/as3-fte-3d.js'));
  globals.NeoFlashFte3D={...fte3dModule,create:settings=>fte3dModule.create({...settings,...options.fte3d})};
  const as3MediaEventsModule=require(resolve(root,'web/as3-media-events.js'));
  globals.NeoFlashAS3MediaEvents={...as3MediaEventsModule,create:settings=>as3MediaEventsModule.create({...settings,...options.as3MediaEvents})};
  const timerModule=require(resolve(root,'web/timers.js'));
  globals.NeoFlashTimers={create:settings=>timerModule.create({...settings,...options.timers})};
  globals.requestAnimationFrame=options.requestAnimationFrame||(callback=>queueMicrotask(callback));
  if(options.keyTarget){globals.addEventListener=options.keyTarget.addEventListener.bind(options.keyTarget);globals.removeEventListener=options.keyTarget.removeEventListener.bind(options.keyTarget);}
  globals.CustomEvent = class { constructor(type, options) { this.type = type; this.detail = options.detail; } };
  globals.dispatchEvent = event => { scriptMessages.push(event.detail); return true; };
  const host = new Function('completeCleanup', 'sourceFrames', 'sourceLocationText', 'formatTraceValue', 'formatTraceArgs', 'emitScriptMessage', 'createScenePainter', 'player', 'functions', 'console', 'sourceKey', 'oneBasedFrame', 'as2Frame', 'createAs2Methods', 'describeThrown', 'unwrapScriptThrow', 'hydrateAsPrelude', 'scriptFunction', 'compileProgramUnit', 'traceSpy', 'global', 'bundle', 'project', 'context', 'font', 'canvas', 'viewport', 'semanticLayer',
    'const proxies = new Map(); const render = () => {}; let lastTrace = "", lastTraceLocation = "";\n' + runtime.slice(start, end) +
    '\nconst originalTrace = trace; trace = (...args) => { traceSpy(...args); return originalTrace(...args); };\n' +
    'initializeFamily(); return { proxyFor, proxyForRef, activateSemantic, dispatchScripts, broadcastFrame, attachNewInstances, resetProject, disposeFamily, stopTimers, display:()=>display, stage:()=>stageHost, as2Stage:()=>as2StageHost, legacy:()=>legacyGlobals, timers:()=>timerHost, binary:()=>binaryHost, net:()=>netHost, bitmap:()=>bitmapHost, text:()=>textHost, filters:()=>filterHost, textInput:()=>textInputHost, audio:()=>audioHost, video:()=>videoHost, xml:()=>xmlHost, loader:()=>loaderHost, amf:()=>amfHost, storage:()=>storageHost, external:()=>externalHost, air:()=>airHost, netSys:()=>as3NetSysHost, fte3d:()=>fte3dHost, mediaEvents:()=>as3MediaEventsHost, program:()=>program, redrawRegions:()=>lastRedrawRects.slice(), computeRedrawRegions, lastTrace: () => lastTrace, lastTraceLocation: () => lastTraceLocation };'
  )(completeCleanup, sourceFrames, sourceLocationText, formatTraceValue, formatTraceArgs, (kind, message, frames) => emitScriptMessage(kind, message, frames, globals), createScenePainter, player, functions, { log: (...args) => hostConsole.push(args) }, sourceKey, oneBasedFrame, as2Frame, createAs2Methods, describeThrown, unwrapScriptThrow, hydrateAsPrelude, scriptFunction, compileProgramUnit, (...args) => traceCalls.push(args),
    globals, options.bundle, options.project || options.bundle?.project, options.context, kind=>`${kind.font_size}px ${JSON.stringify(kind.font_family)}, sans-serif`, options.canvas, options.keyboardTarget, options.semanticLayer);
  return { host, traceCalls, hostConsole, scriptConsole, scriptMessages };
}

function createHostHarness(root, { language, source }, code) {
  const script = { language, source };
  const lifetimes = new Map([['[]', 1], ['["blue"]', 2], ['["red"]', 3]]);
  let queue = [];
  const player = {
    lifetime_id: key => lifetimes.get(key) || 0,
    child_path: (key, name) => JSON.stringify(key === '[]' && lifetimes.has(JSON.stringify([name])) ? [name] : null),
    clip_frame: () => 0, clip_frame_count: () => 1,
    property: () => 0, set_property: () => true, control: () => true, resolve_frame: () => -1,
    scripts_json: () => { const events = queue; queue = []; return JSON.stringify(events); },
    is_dispatch_current: raw => { const item = JSON.parse(raw); return lifetimes.get(JSON.stringify(item.target_path)) === item.target_lifetime; },
    script_overflowed: () => false,
  };
  const result = createRuntimeHost(root, player, [{ script, code, location: 'AS3 keyboard integration' }]);
  const { host } = result;
  function click(name) {
    queue.push({ target_path: [name], target_lifetime: lifetimes.get(JSON.stringify([name])), event: 'click', script });
    host.dispatchScripts([name]);
  }
  return { ...result, click, lifetimes, enqueue: item => queue.push(item), script };
}

export function verifyHostIntegration(root, scriptFixture, code) {
  const { host, click, lifetimes, enqueue, script, traceCalls, hostConsole, scriptConsole } = createHostHarness(root, scriptFixture, code);
  click('blue');
  const blue = host.proxyFor(['blue']), savedIncrement = blue.increment;
  assert.equal(host.lastTrace(), 'Blue 1');
  click('blue');
  assert.equal(host.lastTrace(), 'Blue 2');
  click('red');
  const red = host.proxyFor(['red']);
  assert.equal(blue.count, 2); assert.equal(red.count, 1);
  assert.equal(savedIncrement.call(red), 3); assert.equal(red.count, 1);
  lifetimes.set('["blue"]', 4);
  const replacement = host.proxyFor(['blue']);
  assert.notEqual(blue, replacement); assert.equal(replacement.count, undefined);
  click('blue'); assert.equal(replacement.count, 1);
  enqueue({ target_path: ['blue'], target_lifetime: 2, event: 'click', script });
  host.dispatchScripts(['blue']); assert.equal(replacement.count, 1);
  assert.equal(savedIncrement(), 4); assert.equal(replacement.count, 1);
  assert.throws(() => blue.x, /removed or replaced/);
  assert.deepEqual(traceCalls, [['Blue', 1], ['Blue', 2], ['Blue', 1], ['Blue', 1]]);
  assert.deepEqual(hostConsole, traceCalls.map(args => ['[NeoFlash]', ...args]));
  assert.deepEqual(scriptConsole, [], 'AS must not call console directly');
  assert.equal(host.lastTrace(), 'Blue 1');
}

export function verifyHostFailureFormatting(root) {
  const hostile = Object.defineProperty({ toString() { throw new Error('secondary conversion failure'); } }, 'message', {
    get() { throw new Error('secondary getter failure'); },
  });
  const revoked = Proxy.revocable({}, {}); revoked.revoke();
  const fixtures = [
    ['reason', 'reason'], [17, '17'], [null, 'null'], [undefined, 'undefined'],
    [false, 'false'], ['', ''], [Symbol('reason'), 'Symbol(reason)'], [3n, '3'],
    [{ detail: 1 }, '[object Object]'], [new Error('original error'), 'original error'],
    [hostile, '<unprintable thrown value>'], [revoked.proxy, '<unprintable thrown value>'],
    [Object.create(null), '<unprintable thrown value>'],
  ];
  for (const [value, description] of fixtures) for (const branded of [false, true]) {
    const { host, click, traceCalls } = createHostHarness(root,
      { language: 'java_script', source: 'throw this.payload;' }, 'throw this.payload;');
    host.proxyFor(['blue']).payload = branded ? { [Symbol.for('neoflash.as2.thrown.v1')]: true, value } : value;
    assert.throws(() => click('blue'), error => {
      assert.equal(error.message, `AS3 keyboard integration: ${description}`);
      assert.equal(error.cause, value, 'diagnostics must retain the exact thrown value');
      return true;
    });
    assert.deepEqual(traceCalls, [], 'failed dispatch must not invent a trace');
  }
}

export async function verifyTryCatchHostErrorsAndStepSentinels(root) {
  const { runCargo } = require(resolve(root, 'scripts/test-cargo.cjs'));
  const { homedir } = await import('node:os');
  const { existsSync } = await import('node:fs');
  const vm = await import('node:vm');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (existsSync(localCargo) ? localCargo : 'cargo');

  const as2CatchSource = [
    'import flash.display.BitmapData;',
    'import flash.geom.Point;',
    'var caught = [];',
    'try { this.hostDisplayThrow(); } catch (err) { caught.push("display:" + err.message); }',
    'try { var badBmp = new BitmapData(0, 0); } catch (err) { caught.push("bitmap:" + (err.message.length > 0)); }',
    'try { this.hostTextThrow(); } catch (err) { caught.push("text:" + err.message); }',
    'try { this.hostSoundThrow(); } catch (err) { caught.push("sound:" + err.message); }',
    'try { this.hostXmlThrow(); } catch (err) { caught.push("xml:" + err.message); }',
    'try { this.hostGeomThrow(); } catch (err) { caught.push("geom:" + err.message); }',
    'try { this.hostCrossRealmThrow(); } catch (err) { caught.push("realm:" + err.message); }',
    'this.caughtSummary = caught.join("|");',
  ].join('\n');

  const as3CatchSource = [
    'import flash.display.BitmapData;',
    'var caught:Array = [];',
    'try { this.hostDisplayThrow(); } catch (err:Error) { caught.push("display:" + err.message); }',
    'try { var badBmp:BitmapData = new BitmapData(0, 0); } catch (err:Error) { caught.push("bitmap:" + (err.message.length > 0)); }',
    'try { this.hostTextThrow(); } catch (err:Error) { caught.push("text:" + err.message); }',
    'try { this.hostSoundThrow(); } catch (err:Error) { caught.push("sound:" + err.message); }',
    'try { this.hostXmlThrow(); } catch (err:*) { caught.push("xml:" + err.message); }',
    'try { this.hostGeomThrow(); } catch (err:Error) { caught.push("geom:" + err.message); }',
    'try { this.hostCrossRealmThrow(); } catch (err:Error) { caught.push("realm:" + err.message); }',
    'this.caughtSummary = caught.join("|");',
  ].join('\n');

  const as2InfLoopSource = 'this.swallowed = false; try { while (true) {} } catch (e) { this.swallowed = true; }';
  const as3InfLoopSource = 'this.swallowed = false; try { while (true) {} } catch (e:*) { this.swallowed = true; }';
  const as2SentinelSource = 'this.swallowed = false; try { this.throwAbortSentinel(); } catch (e) { this.swallowed = true; }';
  const as3SentinelSource = 'this.swallowed = false; try { this.throwAbortSentinel(); } catch (e:Error) { this.swallowed = true; }';

  const project = {
    format_version: 2,
    name: 'FO-03 try/catch host error fixture',
    width: 320,
    height: 180,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'scene',
    symbols: [{ id: 'scene', name: 'Scene', kind: 'movie_clip', timeline: { frame_count: 1, layers: [], scripts: [] } }],
    sources: [],
  };
  const scripts = [
    { language: 'action_script2', source: as2CatchSource },
    { language: 'action_script3', source: as3CatchSource },
    { language: 'action_script2', source: as2InfLoopSource },
    { language: 'action_script3', source: as3InfLoopSource },
    { language: 'action_script2', source: as2SentinelSource },
    { language: 'action_script3', source: as3SentinelSource },
  ];
  const res = runCargo(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-script', '--example', 'compile_project'], {
    cwd: root,
    input: JSON.stringify({ project, scripts }),
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
  });
  assert.equal(res.status, 0, res.error?.message || res.stderr);
  const compiled = JSON.parse(res.stdout).Ok;
  assert.ok(compiled, res.stdout);

  const crossRealmErr = vm.runInNewContext('new TypeError("cross-realm failure")');
  function installThrowers(clip) {
    clip.hostDisplayThrow = () => { throw new Error('swapDepths host error'); };
    clip.hostTextThrow = () => { throw new RangeError('TextField host error'); };
    clip.hostSoundThrow = () => { throw new Error('Sound host error'); };
    clip.hostXmlThrow = () => { throw new SyntaxError('XML host error'); };
    clip.hostGeomThrow = () => { throw new TypeError('Geometry host error'); };
    clip.hostCrossRealmThrow = () => { throw crossRealmErr; };
    clip.throwAbortSentinel = () => {
      const abortErr = new Error('Host VM abort sentinel');
      Object.defineProperty(abortErr, '__nfUncatchable', { value: true });
      throw abortErr;
    };
  }

  // 1. AS2 & AS3 catch ordinary host/intrinsic/cross-realm Errors cleanly.
  for (const idx of [0, 1]) {
    const player = new runtimeWasm.WebPlayer(JSON.stringify(project));
    const unit = { script: scripts[idx], code: compiled.scripts[idx], location: `fo03-${idx}` };
    const env = createRuntimeHost(root, player, [unit], {
      bundle: { project, program: { code: compiled.code, class_names: [] }, scripts: [{ ...scripts[idx], code: compiled.scripts[idx] }] },
      project,
    });
    const blue = env.host.proxyFor([]);
    installThrowers(blue);
    player.free();

    const harness = createHostHarness(root, scripts[idx], compiled.scripts[idx]);
    const target = harness.host.proxyFor(['blue']);
    installThrowers(target);
    harness.click('blue');
    assert.equal(
      target.caughtSummary,
      'display:swapDepths host error|bitmap:true|text:TextField host error|sound:Sound host error|xml:XML host error|geom:Geometry host error|realm:cross-realm failure',
      `script[${idx}] should catch all host/intrinsic/cross-realm errors`,
    );
  }

  // 2. Infinite loop step-limit sentinels CANNOT be swallowed by AS2 or AS3 try/catch!
  const prevSteps = globalThis.__nfMaxScriptSteps;
  globalThis.__nfMaxScriptSteps = 256;
  try {
    for (const idx of [2, 3]) {
      const harness = createHostHarness(root, scripts[idx], compiled.scripts[idx]);
      const target = harness.host.proxyFor(['blue']);
      assert.throws(
        () => harness.click('blue'),
        /Script step limit exceeded/,
        `script[${idx}] infinite loop inside try/catch must abort instead of being swallowed`,
      );
      assert.equal(target.swallowed, false, `script[${idx}] catch block must not run on step overflow`);
    }
  } finally {
    if (prevSteps === undefined) delete globalThis.__nfMaxScriptSteps;
    else globalThis.__nfMaxScriptSteps = prevSteps;
  }

  // 3. Explicit __nfUncatchable VM abort sentinels rethrow past AS2 and AS3 catch blocks!
  for (const idx of [4, 5]) {
    const harness = createHostHarness(root, scripts[idx], compiled.scripts[idx]);
    const target = harness.host.proxyFor(['blue']);
    installThrowers(target);
    assert.throws(
      () => harness.click('blue'),
      /Host VM abort sentinel/,
      `script[${idx}] uncatchable sentinel must rethrow past user catch`,
    );
    assert.equal(target.swallowed, false);
  }
}

const isMainModule = (() => {
  try {
    const { fileURLToPath } = require('node:url');
    return Boolean(process.argv[1]) && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
  } catch (_) {
    return false;
  }
})();

if (isMainModule) {
  const { dirname } = require('node:path');
  const { fileURLToPath } = require('node:url');
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
  verifyHostFailureFormatting(repoRoot);
  await verifyTryCatchHostErrorsAndStepSentinels(repoRoot);
  console.log('host-integration: failure formatting, externalized as-intrinsics, AS2/AS3 host try/catch, and uncatchable step sentinels passed.');
}
