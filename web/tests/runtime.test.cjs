const assert = require('node:assert/strict');
const test = require('node:test');
const {
  localPoint,
  hitShape,
  oneBasedFrame,
  sourceKey,
  ensureBundleFontsLoaded,
  mountAfterBundleFonts,
} = require('../runtime.js');

test('bundle font readiness ignores unrelated host faces and gates child mount', async () => {
  let releaseBundleFace;
  let bundleLoads = 0;
  let unrelatedLoads = 0;
  let unrelatedVariantLoads = 0;
  const bundleFace = {
    family: '"NeoFlash Child Font"',
    style: 'normal',
    weight: '400',
    status: 'unloaded',
    load() {
      bundleLoads++;
      return new Promise(resolve => {
        releaseBundleFace = () => {
          this.status = 'loaded';
          resolve(this);
        };
      });
    },
  };
  const unrelatedFace = {
    family: '"Host Page Font"',
    style: 'normal',
    weight: '400',
    status: 'unloaded',
    load() {
      unrelatedLoads++;
      return new Promise(() => {});
    },
  };
  const unrelatedVariant = {
    family: '"NeoFlash Child Font"',
    style: 'italic',
    weight: '700',
    status: 'unloaded',
    load() {
      unrelatedVariantLoads++;
      return new Promise(() => {});
    },
  };
  const inserted = [];
  const fontSet = {
    ready: new Promise(() => {}),
    *[Symbol.iterator]() {
      yield unrelatedFace;
      yield unrelatedVariant;
      yield bundleFace;
    },
  };
  const doc = {
    fonts: fontSet,
    head: { appendChild: node => inserted.push(node) },
    querySelectorAll: () => [],
    createElement: () => ({ setAttribute() {}, textContent: '' }),
  };
  const css = '@font-face { font-family: "NeoFlash Child Font"; font-style: normal; font-weight: 400; src: url(data:font/ttf;base64,AA==); }';

  const loading = ensureBundleFontsLoaded(doc, css);
  await Promise.resolve();
  assert.equal(bundleLoads, 1, 'the face introduced by this bundle starts loading');
  assert.equal(unrelatedLoads, 0, 'an unrelated host-page face is never forced to load');
  assert.equal(unrelatedVariantLoads, 0, 'an unrelated style in the same family is never forced to load');
  assert.equal(inserted.length, 1, 'missing bundle CSS is injected once');
  releaseBundleFace();
  await loading;

  bundleFace.status = 'unloaded';
  releaseBundleFace = null;
  let current = true;
  let mounted = 0;
  const cancelled = mountAfterBundleFonts(
    doc,
    { fonts: { css_rules: css } },
    () => {
      mounted++;
      return { contentRef: ['child'] };
    },
    () => current,
  );
  await Promise.resolve();
  assert.equal(mounted, 0, 'child mount waits for its embedded face');
  current = false;
  releaseBundleFace();
  assert.equal(await cancelled, null, 'a cancelled child is not mounted after its font resolves');
  assert.equal(mounted, 0);

  bundleFace.status = 'unloaded';
  releaseBundleFace = null;
  current = true;
  const mountedResult = mountAfterBundleFonts(
    doc,
    { fonts: { css_rules: css } },
    () => {
      mounted++;
      return { contentRef: ['child'] };
    },
    () => current,
  );
  await Promise.resolve();
  assert.equal(mounted, 0);
  releaseBundleFace();
  assert.deepEqual(await mountedResult, { contentRef: ['child'] });
  assert.equal(mounted, 1, 'active child mounts only after its own font is ready');
  assert.equal(unrelatedLoads, 0);
  assert.equal(unrelatedVariantLoads, 0);

  let syncMounted = false;
  const syncResult = mountAfterBundleFonts(doc, { fonts: { css_rules: '' } }, () => {
    syncMounted = true;
    return { contentRef: ['fontless-child'] };
  });
  assert.equal(syncMounted, true, 'a fontless child keeps the existing synchronous mount path');
  assert.deepEqual(syncResult, { contentRef: ['fontless-child'] });
});

test('bundle font parsing preserves quoted delimiters and exporter escapes', async () => {
  const family = 'Semi; {Brace} Quote " Slash \\ Family';
  const escapedFamily = family.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  let matchingLoads = 0;
  let fallbackLoads = 0;
  const matchingFace = {
    family: `"${escapedFamily}"`,
    style: 'italic',
    weight: '700',
    status: 'unloaded',
    load() {
      matchingLoads++;
      return Promise.resolve(this);
    },
  };
  const css =
    `@font-face { font-family: "${escapedFamily}"; font-style: italic; font-weight: 700; ` +
    `src: url('data:font/ttf;variant=a}b;base64,AA=='); font-display: block; }`;
  const doc = {
    fonts: {
      *[Symbol.iterator]() { yield matchingFace; },
      load() {
        fallbackLoads++;
        return new Promise(() => {});
      },
    },
  };

  await ensureBundleFontsLoaded(doc, css);
  assert.equal(matchingLoads, 1, 'quoted semicolons, braces, quotes and backslashes preserve the exact family match');
  assert.equal(fallbackLoads, 0, 'an exact parsed descriptor does not fall back to a broad family lookup');
});

test('inverse transforms rotated/scaled nested display coordinates', () => {
  const local = localPoint([0, 2, -3, 0, 100, 200], 85, 208);
  assert.deepEqual(local, [4,5]);
  assert.equal(localPoint([0,0,0,0,10,20],12,22), null);
});
test('ellipse picking excludes bounding box corners', () => {
  const ellipse={type:'ellipse',width:100,height:50};
  assert.equal(hitShape(ellipse,0,0),false);
  assert.equal(hitShape(ellipse,50,25),true);
  assert.equal(hitShape(ellipse,100,25),true);
  const transparent={...ellipse,fill:[50,100,150,0],stroke:[0,0,0,0],stroke_width:4};
  assert.equal(hitShape(transparent,50,25),false,'transformed alpha controls hit testing even when draw.alpha is one');
  assert.equal(hitShape({...transparent,fill:[50,100,150,128]},50,25),true,'alpha offsets can restore visible paint');
  assert.equal(hitShape({type:'rectangle',width:100,height:50,fill:[0,0,0,0],stroke:[0,0,0,0],stroke_width:0},50,25),false);
});
test('frame controls are one-based, labels take precedence, invalid targets fail', () => {
  assert.equal(oneBasedFrame(1,24,()=>-1),0);
  assert.equal(oneBasedFrame('intro',24,()=>7),7);
  assert.equal(oneBasedFrame('12',24,()=>-1),11);
  assert.throws(()=>oneBasedFrame(0,24,()=>-1),/between 1 and 24/);
  assert.throws(()=>oneBasedFrame(25,24,()=>-1),/between 1 and 24/);
  assert.throws(()=>oneBasedFrame('missing',24,()=>-1),/does not exist/);
});
test('compiled source lookup distinguishes dialect and exact source',()=>{
  assert.notEqual(sourceKey({language:'java_script',source:'play()'}),sourceKey({language:'action_script2',source:'play()'}));
});

test('runtime source maps calibrate wrappers, select authored calls and tolerate hostile exceptions', () => {
  const {functionBodyLineOffset, scriptFunction, mapScriptStack, mappedErrorFrames} = require('../runtime.js');
  const offset = functionBodyLineOffset();
  assert.ok(Number.isInteger(offset));
  const script = {
    source_url: 'neoflash-script-2.js', code: 'trace();\nthrow new Error("authored");',
    source_locations: [
      {generated_line:1,generated_column:1,source:'Scene / frame 9',line:12,column:3},
      {generated_line:2,generated_column:1,source:'Scene / frame 9',line:16,column:5},
    ],
  };
  let traced;
  try {
    scriptFunction(script, ['trace'])(() => {traced=mapScriptStack(new Error().stack,[script],offset);});
    assert.fail('expected authored error');
  } catch (error) {
    assert.equal(mappedErrorFrames(error,[script],offset)[0].line,16);
    assert.equal(mappedErrorFrames(new Error('wrapper',{cause:error}),[script],offset)[0].column,5);
  }
  assert.equal(traced[0].line,12);
  assert.deepEqual(mapScriptStack('at helper (unrelated.js:2:1)',[script],offset),[]);
  assert.deepEqual(mapScriptStack('Error: neoflash-script-2.js:'+(offset+3)+':1',[script],offset),[], 'error message cannot impersonate a stack frame');
  assert.deepEqual(mapScriptStack('at helper (neoflash-script-2.js:'+(offset+2+30)+':1)',[script],offset),[]);
  let reads=0;
  const hostile={get stack(){reads++;throw new Error('getter');},get cause(){reads++;throw new Error('getter');}};
  assert.deepEqual(mappedErrorFrames(hostile,[script],offset),[]);
  assert.equal(reads,0,'mapping never invokes authored stack/cause getters');
  assert.deepEqual(mappedErrorFrames('primitive throw',[script],offset),[]);
  const cycle={};cycle.cause=cycle;
  assert.deepEqual(mappedErrorFrames(cycle,[script],offset),[]);
  assert.deepEqual(mappedErrorFrames(new Proxy({}, {getOwnPropertyDescriptor(){throw new Error('proxy');}}),[script],offset),[]);
});

test('AS3 navigateToURL and URLRequest handle web links, window targets, POST variables and mailto:',()=>{
  const NeoFlashEvents = require('../events.js');
  const NeoFlashByteArray = require('../byte-array.js');
  const NeoFlashNet = require('../net.js');
  const NeoFlashExternal = require('../external.js');

  const assigned = [];
  const opened = [];
  const external = NeoFlashExternal.create({
    hostObject: {},
    hostGlobal: {
      location: {assign: url => assigned.push(url)},
      open: (url, target) => { opened.push([url, target]); return {}; },
    },
  });
  const net = NeoFlashNet.create({
    events: NeoFlashEvents,
    binary: NeoFlashByteArray.create(),
    baseURL: 'https://neoflash.example/app/index.html',
    navigate: prepared => external.navigate(prepared),
  });

  // 1. Default window opens `_blank` for HTTP(S); `_self` uses `location.assign`.
  net.navigateToURL(new net.URLRequest('https://example.org/docs'));
  assert.deepEqual(opened, [['https://example.org/docs', '_blank']]);
  net.navigateToURL(new net.URLRequest('about.html'), '_self');
  assert.deepEqual(assigned, ['https://neoflash.example/app/about.html']);

  // 2. `mailto:` works with URLRequest + URLVariables query parameters and rejects POST.
  const mailReq = new net.URLRequest('mailto:team@example.org');
  const vars = new net.URLVariables();
  vars.subject = 'NeoFlash';
  mailReq.data = vars;
  net.navigateToURL(mailReq);
  assert.equal(assigned.at(-1), 'mailto:team@example.org?subject=NeoFlash');

  const postMail = new net.URLRequest('mailto:team@example.org');
  postMail.method = net.URLRequestMethod.POST;
  assert.throws(() => net.navigateToURL(postMail), /mailto: navigation supports GET only/);
  assert.throws(() => net.navigateToURL(new net.URLRequest('javascript:alert(1)')), /HTTP\(S\)/);
});

test('NeoFlashAudio timeline honors in_point and out_point trim windows and loop bounds', async () => {
  // Two seconds of 16-bit mono 8 kHz WAV matches the decoder's buffer below.
  const wav = Buffer.alloc(44 + 32000);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(36 + 32000, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(8000, 24);
  wav.writeUInt32LE(16000, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(32000, 40);

  const sources = [];
  let time = 1.0;
  const param = () => ({ value: 1, setValueAtTime() {}, linearRampToValueAtTime() {} });
  const node = kind => ({ kind, gain: param(), connect() {}, disconnect() {} });
  const context = {
    state: 'suspended',
    sampleRate: 8000,
    get currentTime() { return time; },
    destination: node('destination'),
    resume() { this.state = 'running'; return Promise.resolve(); },
    close() { return Promise.resolve(); },
    decodeAudioData() { return Promise.resolve({ duration: 2.0, length: 16000, numberOfChannels: 1 }); },
    createGain() { return node('gain'); },
    createStereoPanner() { return { ...node('pan'), pan: param() }; },
    createChannelSplitter() { return node('splitter'); },
    createChannelMerger() { return node('merger'); },
    createBufferSource() {
      const source = {
        ...node('source'),
        loop: false,
        loopStart: 0,
        loopEnd: 0,
        start(when, offset) { this.started = [when, offset]; },
        stop(when) { this.stopped = when ?? time; },
      };
      sources.push(source);
      return source;
    },
  };
  const audio = require('../audio.js').create({
    events: require('../events.js'),
    net: { URLLoader: class {}, mediaSource: () => ({ bytes: new Uint8Array(wav) }) },
    binary: { toBytes: x => x },
    resources: [{ id: 'clip', path: 'audio/clip.wav', media_type: 'audio/wav' }],
    createContext: () => context,
    isUserGesture: () => true,
    now: () => time,
    onError: error => { throw error; },
  });

  audio.preloadTimeline(['clip']);
  await new Promise(done => setTimeout(done, 0));
  await audio.resumeFromGesture();

  audio.timeline([{
    op: 'play',
    resource: 'clip',
    sync: 'event',
    loops: 2,
    loop_forever: false,
    in_point: 0.25,
    out_point: 0.75,
    envelope: [[0, 1, 1]],
  }]);
  assert.equal(sources.length, 1);
  assert.deepEqual(sources[0].started, [1.0, 0.25]);
  assert.equal(sources[0].loop, true);
  assert.equal(sources[0].loopStart, 0.25);
  assert.equal(sources[0].loopEnd, 0.75);
  assert.ok(Math.abs(sources[0].stopped - 2.0) < 1e-9, '2 loops of 0.5 s trim window stops at t=2.0');
  audio.dispose();
});

test('NeoFlashAudio timeline releases failed PCM reservations and retires replaced or restarted decodes', async () => {
  const pending=[],errors=[],sources=[];
  let failState=false,closed=0;
  const cleanupError=new Error('first audio state cleanup failure');
  const node=()=>({gain:{value:1},connect(){},disconnect(){}});
  const context={
    state:'suspended',sampleRate:8000,currentTime:0,destination:node(),
    resume(){this.state='running';return Promise.resolve();},close(){closed++;return Promise.resolve();},
    createGain:node,createStereoPanner:node,
    decodeAudioData(){return new Promise((resolve,reject)=>pending.push({resolve,reject}));},
    createBufferSource(){const source={...node(),start(){},stop(){}};sources.push(source);return source;},
  };
  const resource={id:'clip',path:'clip.wav'};
  const audio=require('../audio.js').create({
    events:require('../events.js'),
    net:{URLLoader:class{},mediaSource:path=>path.startsWith('http:')?{kind:'http',url:path}:{bytes:new Uint8Array([1])}},
    binary:{toBytes:value=>value},formats:{inspect:()=>({decodedBytes:64})},
    resources:[resource],maxDecodedBytes:128,createContext:()=>context,
    isUserGesture:()=>true,now:()=>0,onError:error=>errors.push(error),
    onState:()=>{if(failState){failState=false;throw cleanupError;}},
  });
  const flush=()=>new Promise(resolve=>setImmediate(resolve));
  const buffer=()=>({duration:1,length:8,numberOfChannels:1});
  try{
    audio.preloadTimeline(['clip']);
    assert.equal(audio.status().decodedBytes,64);
    const original=new Error('original native decode failure');
    pending.shift().reject(original);await flush();
    assert.equal(errors[0],original,'native decode failure retains its original value');
    assert.equal(audio.status().decodedBytes,0,'failed decode releases its complete reservation');

    audio.unregisterResources(['clip']);audio.registerResources([resource]);audio.preloadTimeline(['clip']);
    const beforeRestart=pending.shift();
    audio.reset();
    assert.equal(audio.status().decodedBytes,64,'unabortable native decoding stays budgeted across Restart');
    audio.preloadTimeline(['clip']);
    const afterRestart=pending.shift();
    assert.equal(audio.status().decodedBytes,128);
    beforeRestart.resolve(buffer());await flush();
    assert.equal(audio.status().decodedBytes,64,'stale decode only releases its own reservation');
    afterRestart.resolve(buffer());await flush();
    assert.equal(audio.status().decodedBytes,32,'successful decode accounts for actual PCM size');
    audio.unregisterResources(['clip']);
    assert.equal(audio.status().decodedBytes,0,'unregister releases committed PCM bytes');

    audio.registerResources([resource]);audio.preloadTimeline(['clip']);
    const removed=pending.shift();
    audio.unregisterResources(['clip']);audio.registerResources([resource]);audio.preloadTimeline(['clip']);
    const replacement=pending.shift();
    await audio.resumeFromGesture();
    audio.timeline([{op:'stream_start',resource:'clip',channel:'music',offset:0,loops:1}]);
    replacement.resolve(buffer());await flush();
    assert.equal(audio.status().active,1);
    const activeSource=sources.at(-1);
    removed.reject(new Error('obsolete resource decode failure'));await flush();
    assert.equal(audio.status().active,1,'an old same-ID completion cannot stop a replacement voice');
    assert.equal(typeof activeSource.onended,'function');
    assert.equal(errors.length,1,'retired resource failure cannot reach the replacement publication');
    assert.equal(audio.status().decodedBytes,32);

    audio.timeline([{op:'stream_start',resource:'clip',channel:'second',offset:0,loops:1}]);
    audio.registerResources([{id:'pending',path:'pending.wav'}]);audio.preloadTimeline(['pending']);
    const cleanupDecode=pending.shift(),sourcesBeforeReset=sources.slice(-2),closedBeforeReset=closed;
    failState=true;
    assert.throws(()=>audio.reset(),error=>error===cleanupError);
    assert.equal(audio.status().active,0,'later voices retire despite a first cleanup callback failure');
    assert.ok(sourcesBeforeReset.every(source=>source.onended===null));
    assert.equal(closed,closedBeforeReset+1,'context closes even after earlier cleanup fails');
    assert.equal(audio.status().decodedBytes,64,'failed cleanup retains only outstanding native decode reservations');
    cleanupDecode.resolve(buffer());await flush();
    assert.equal(audio.status().decodedBytes,0);
    audio.preloadTimeline(['clip']);pending.shift().resolve(buffer());await flush();
    await audio.resumeFromGesture();
    audio.timeline([{op:'stream_start',resource:'clip',channel:'retry',offset:0,loops:1}]);
    assert.equal(audio.status().active,1,'fresh playback works after the failed cleanup attempt');

    const originalFetch=global.fetch,requests=[];
    global.fetch=(url,options)=>new Promise(resolve=>requests.push({url,signal:options.signal,resolve}));
    try{
      audio.registerResources([{id:'remote',path:'http://example.test/audio.wav'}]);audio.preloadTimeline(['remote']);
      audio.reset();
      assert.equal(requests[0].signal.aborted,true,'Restart aborts pending timeline HTTP transfers');
      requests[0].resolve({ok:true,arrayBuffer:async()=>new Uint8Array([1]).buffer});await flush();
      assert.equal(pending.length,0,'an aborted old response cannot schedule a native decode');
      audio.preloadTimeline(['remote']);audio.unregisterResources(['remote']);
      assert.equal(requests[1].signal.aborted,true,'resource retirement aborts its pending transfer');
      requests[1].resolve({ok:true,arrayBuffer:async()=>new Uint8Array([1]).buffer});await flush();
      assert.equal(pending.length,0);
    }finally{global.fetch=originalFetch;}
    audio.preloadTimeline(['clip']);pending.shift().resolve(buffer());await flush();
    await audio.resumeFromGesture();audio.timeline([{op:'stream_start',resource:'clip',channel:'dispose',offset:0,loops:1}]);
    failState=true;
    assert.throws(()=>audio.dispose(),error=>error===cleanupError);
    assert.equal(audio.status().active,0);
    assert.throws(()=>audio.preloadTimeline(['clip']),/disposed/,'failed disposal still retires the service permanently');
  }finally{audio.dispose();}
  assert.equal(audio.status().decodedBytes,0);
});

test('NeoFlashPreview.layout supports Fit, 100% (No scale), Exact fit (stretch) and Fill (crop)', () => {
  const { layout } = require('../preview.js');
  const fit = layout(800, 600, 1000, 500, 'fit', 16);
  assert.ok(Math.abs(fit.stageWidth - 624) < 1e-6);
  assert.ok(Math.abs(fit.stageHeight - 468) < 1e-6);

  const noScale = layout(800, 600, 1000, 500, '1', 16);
  assert.equal(noScale.stageWidth, 800);
  assert.equal(noScale.stageHeight, 600);

  const stretch = layout(800, 600, 1000, 500, 'stretch', 16);
  assert.equal(stretch.stageWidth, 968);
  assert.equal(stretch.stageHeight, 468);
  assert.notEqual(stretch.scale, stretch.scaleY);

  const fill = layout(800, 600, 1000, 500, 'fill', 16);
  assert.equal(fill.stageWidth, 968);
  assert.equal(fill.stageHeight, 726);
});

test('Compact and Balanced packages boot directly over file:// using NeoFlashWasmBase64 and honor player_config chrome/playback options', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');

  const runtimeJs = fs.readFileSync(path.resolve(__dirname, '../runtime.js'), 'utf8');
  const previewJs = fs.readFileSync(path.resolve(__dirname, '../preview.js'), 'utf8');
  const pathJs = fs.readFileSync(path.resolve(__dirname, '../path.js'), 'utf8');
  const accessibilityJs = fs.readFileSync(path.resolve(__dirname, '../accessibility.js'), 'utf8');
  const eventsJs = fs.readFileSync(path.resolve(__dirname, '../events.js'), 'utf8');
  const stageJs = fs.readFileSync(path.resolve(__dirname, '../stage.js'), 'utf8');
  const displayJs = fs.readFileSync(path.resolve(__dirname, '../display.js'), 'utf8');

  function makeElement(id = '') {
    const attrs = new Map();
    return {
      id,
      hidden: false,
      disabled: false,
      textContent: '',
      title: '',
      value: 'fit',
      tabIndex: 0,
      clientWidth: 960,
      clientHeight: 600,
      scrollLeft: 0,
      scrollTop: 0,
      dataset: {},
      ownerDocument: { activeElement: null },
      style: { display: 'none' },
      classList: { toggle() {} },
      setAttribute(k, v) { attrs.set(k, String(v)); },
      getAttribute(k) { return attrs.get(k) ?? null; },
      removeAttribute(k) { attrs.delete(k); },
      addEventListener() {},
      removeEventListener() {},
      replaceChildren() {},
      appendChild() {},
      getBoundingClientRect() { return { left: 0, top: 0, width: 960, height: 600 }; },
      getContext() {
        return {
          setTransform() {},
          clearRect() {},
          fillRect() {},
          save() {},
          restore() {},
          transform() {},
          beginPath() {},
          arc() {},
          fill() {},
          stroke() {},
          fillText() {},
        };
      },
    };
  }

  const elements = new Map([
    ['stage', makeElement('stage')],
    ['stage-space', makeElement('stage-space')],
    ['stage-surface', makeElement('stage-surface')],
    ['semantic-layer', makeElement('semantic-layer')],
    ['viewport', makeElement('viewport')],
    ['status', makeElement('status')],
    ['toggle', makeElement('toggle')],
    ['restart', makeElement('restart')],
    ['zoom', makeElement('zoom')],
    ['pan', makeElement('pan')],
    ['audio-toggle', makeElement('audio-toggle')],
    ['report-toggle', makeElement('report-toggle')],
    ['name', makeElement('name')],
    ['load-info', makeElement('load-info')],
    ['audio-status', makeElement('audio-status')],
    ['trace', makeElement('trace')],
    ['size-report', makeElement('size-report')],
    ['error', makeElement('error')],
    ['loading', makeElement('loading')],
  ]);
  const footer = makeElement('footer');
  const transport = makeElement('transport');
  const body = makeElement('body');

  let receivedWasmBytes = null;
  let playingState = true;
  let scriptsQueue = [{
    script: {
      language: 'java_script',
      source: 'trace("hello from compact file://");',
    },
    event: 'frame',
    target_path: [],
  }];
  const mockPlayer = {
    current_frame: () => 0,
    frame_count: () => 4,
    total_frames: () => 4,
    is_playing: () => playingState,
    set_playing(v) { playingState = Boolean(v); },
    tick() { return true; },
    reset() { playingState = true; },
    scene_json: () => '[]',
    scripts_json: () => {
      const next = scriptsQueue;
      scriptsQueue = [];
      return JSON.stringify(next);
    },
    accessibility_json: () => '[]',
    is_dispatch_current: () => true,
    script_overflowed: () => false,
    lifetime_id: () => 'root-life',
  };

  const sandbox = {
    console: { warn() {}, info() {}, error() {} },
    location: { protocol: 'file:', href: 'file:///Users/miles/Projects/NeoFlash/examples/test/neoflash-export/index.html' },
    atob: b64 => Buffer.from(b64, 'base64').toString('binary'),
    Uint8Array,
    Map,
    WeakMap,
    Set,
    Promise,
    Error,
    TypeError,
    Math,
    Number,
    String,
    Boolean,
    Array,
    Object,
    JSON,
    Symbol,
    Reflect,
    Path2D: class { moveTo() {} lineTo() {} closePath() {} },
    ResizeObserver: class { observe() {} },
    requestAnimationFrame() {},
    addEventListener() {},
    devicePixelRatio: 1,
    document: {
      body,
      visibilityState: 'visible',
      getElementById: id => elements.get(id) || makeElement(id),
      querySelector: sel => sel === 'footer' ? footer : sel === '.transport' ? transport : null,
      addEventListener() {},
    },
    NeoFlashWasmBase64: Buffer.from('\0asm\x01\0\0\0', 'binary').toString('base64'),
    NeoFlashWasm: Object.assign(
      opts => {
        receivedWasmBytes = opts.module_or_path;
        return Promise.resolve();
      },
      {
        WebPlayer: function() { return mockPlayer; },
      }
    ),
    NeoFlashBundle: {
      project: {
        format_version: 1,
        name: 'Ideas in motion',
        width: 960,
        height: 600,
        fps: 24,
        background: [19, 24, 36, 255],
        root: 'root',
        symbols: [{ id: 'root', name: 'Scene 1', kind: 'movie_clip', timeline: { frame_count: 4, layers: [], scripts: [] } }],
      },
      scripts: [{
        language: 'java_script',
        source: 'trace("hello from compact file://");',
        code: 'trace("hello from compact file://");',
        location: 'Scene 1 / frame 1',
        locations: ['Scene 1 / frame 1'],
        source_url: 'neoflash-script-0.js',
        source_locations: [],
      }],
      wasm_url: 'neoflash_web_bg.wasm',
      player_config: {
        template: 'responsive',
        scale_mode: 'no_scale',
        autoplay: false,
        loop_timeline: false,
        preloader: false,
        include_trace: false,
        chrome: {
          clean: true,
          show_fps: false,
          show_dimensions: false,
          show_controls: false,
          show_load_summary: false,
          show_trace: false,
        },
      },
      publish: { level: 'compact', omitted_modules: ['AIR desktop APIs'] },
      warnings: [],
    },
  };
  sandbox.globalThis = sandbox;
  sandbox.window = sandbox;
  const ctx = vm.createContext(sandbox);
  for (const src of [pathJs, accessibilityJs, previewJs, eventsJs, stageJs, displayJs, runtimeJs]) {
    vm.runInContext(src, ctx);
  }
  await new Promise(resolve => setTimeout(resolve, 10));

  assert.equal(elements.get('error').style.display, 'none', `unexpected boot error: ${elements.get('error').textContent}`);
  assert.ok(receivedWasmBytes instanceof Uint8Array && receivedWasmBytes.length === 8, 'file:// boot decoded NeoFlashWasmBase64 into Uint8Array');
  assert.equal(playingState, false, 'player_config.autoplay=false starts paused');
  assert.equal(body.getAttribute('data-clean'), 'true', 'clean publish sets data-clean=true on body');
  assert.equal(footer.hidden, true, 'clean publish hides technical footer');
  assert.equal(elements.get('trace').textContent, '', 'include_trace=false suppresses trace() DOM output');
});

test('runtime merges authored accessibility into one visible semantic target per lifetime as frames advance', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');

  const runtimeJs = fs.readFileSync(path.resolve(__dirname, '../runtime.js'), 'utf8');
  const previewJs = fs.readFileSync(path.resolve(__dirname, '../preview.js'), 'utf8');
  const pathJs = fs.readFileSync(path.resolve(__dirname, '../path.js'), 'utf8');
  const accessibilityJs = fs.readFileSync(path.resolve(__dirname, '../accessibility.js'), 'utf8');
  const eventsJs = fs.readFileSync(path.resolve(__dirname, '../events.js'), 'utf8');
  const stageJs = fs.readFileSync(path.resolve(__dirname, '../stage.js'), 'utf8');
  const displayJs = fs.readFileSync(path.resolve(__dirname, '../display.js'), 'utf8');

  const mockDocument = {activeElement:null,createElement:tag=>makeElement('',tag)};
  function makeElement(id = '', tag = 'div') {
    const attrs = new Map();
    const children = [];
    return {
      id,
      tagName:tag.toUpperCase(),
      parentNode:null,
      listeners:{},
      hidden: false,
      disabled: false,
      textContent: '',
      title: '',
      value: 'fit',
      get tabIndex() {return attrs.has('tabindex') ? Number(attrs.get('tabindex')) : (tag === 'button' ? 0 : -1);},
      set tabIndex(value) {attrs.set('tabindex',String(value));},
      clientWidth: 800,
      clientHeight: 600,
      scrollLeft: 0,
      scrollTop: 0,
      dataset: {},
      children,
      ownerDocument: mockDocument,
      style: { display: 'none' },
      classList: { toggle() {} },
      setAttribute(k, v) {
        attrs.set(k, String(v));
        if (k === 'tabindex') this.tabIndex = Number(v);
      },
      getAttribute(k) { return attrs.get(k) ?? null; },
      removeAttribute(k) { attrs.delete(k); },
      addEventListener(name,handler) {this.listeners[name]=handler;},
      removeEventListener() {},
      replaceChildren(...next) {
        for(const child of children.slice())child.remove();
        for(const child of next)this.appendChild(child);
      },
      appendChild(child) {
        this.insertBefore(child,null);return child;
      },
      insertBefore(child,before) {child.remove();children.splice(before?children.indexOf(before):children.length,0,child);child.parentNode=this;},
      removeChild(child) {child.remove();},
      remove() {if(this.parentNode){const siblings=this.parentNode.children;siblings.splice(siblings.indexOf(this),1);this.parentNode=null;this.blur();}},
      focus() {mockDocument.activeElement=this;this.listeners.focus?.();},
      blur() {if(mockDocument.activeElement===this)mockDocument.activeElement=null;},
      scrollIntoView() {},
      get firstElementChild() {return children[0]||null;},
      get nextElementSibling() {return this.parentNode?.children[this.parentNode.children.indexOf(this)+1]||null;},
      getBoundingClientRect() { return { left: 0, top: 0, width: 800, height: 600 }; },
      getContext() {
        return {
          setTransform() {},
          clearRect() {},
          fillRect() {},
          save() {},
          restore() {},
          transform() {},
          beginPath() {},
          rect() {},
          arc() {},
          fill() {},
          stroke() {},
          fillText() {},
          measureText(str) { return { width: String(str).length * 8 }; },
        };
      },
    };
  }

  const overlayEl = makeElement('stage-a11y-overlay');
  const elements = new Map([
    ['stage', makeElement('stage')],
    ['stage-space', makeElement('stage-space')],
    ['stage-surface', makeElement('stage-surface')],
    ['semantic-layer', makeElement('semantic-layer')],
    ['stage-a11y-overlay', overlayEl],
    ['viewport', makeElement('viewport')],
    ['status', makeElement('status')],
    ['toggle', makeElement('toggle')],
    ['restart', makeElement('restart')],
    ['zoom', makeElement('zoom')],
    ['pan', makeElement('pan')],
    ['audio-toggle', makeElement('audio-toggle')],
    ['report-toggle', makeElement('report-toggle')],
    ['name', makeElement('name')],
    ['load-info', makeElement('load-info')],
    ['audio-status', makeElement('audio-status')],
    ['trace', makeElement('trace')],
    ['size-report', makeElement('size-report')],
    ['error', makeElement('error')],
    ['loading', makeElement('loading')],
  ]);
  const footer = makeElement('footer');
  const transport = makeElement('transport');
  const body = makeElement('body');

  let activeFrame = 0;
  let sceneVisible = true;
  let activations = 0;
  let playingState = true;
  let rafCallback = null;
  const lifetimes = new Map([
    ['[]', 'root-life'],
    ['["start_btn"]', 'start-life-1'],
    ['["intro_heading"]', 'heading-life-1'],
    ['["victory_modal"]', 'dialog-life-1'],
  ]);
  const runtimeAccessibility = () => activeFrame === 0
    ? [
        {
          instance_path: ['start_btn'],
          lifetime_id: lifetimes.get('["start_btn"]'),
          accessibility: {
            name: 'Begin Quest', description: 'Starts level 1', role: 'button',
            tab_order: 2, disabled: false,
          },
        },
        {
          instance_path: ['intro_heading'],
          lifetime_id: lifetimes.get('["intro_heading"]'),
          accessibility: {
            name: 'Welcome Hero Heading', description: 'Main screen banner', role: 'image',
            tab_order: 1, disabled: false,
          },
        },
      ]
    : [{
        instance_path: ['victory_modal'],
        lifetime_id: lifetimes.get('["victory_modal"]'),
        accessibility: {
          name: 'Mission Complete', description: 'All objectives cleared', role: 'image',
          tab_order: 1, disabled: false,
        },
      }];

  const mockPlayer = {
    current_frame: () => activeFrame,
    frame_count: () => 2,
    total_frames: () => 2,
    is_playing: () => playingState,
    set_playing(v) { playingState = Boolean(v); },
    tick() {
      activeFrame = (activeFrame + 1) % 2;
      return true;
    },
    reset() {
      activeFrame = 0;
      playingState = true;
    },
    scene_json: () => JSON.stringify(sceneVisible ? runtimeAccessibility().map((node,index)=>({instance_path:node.instance_path,transform:[1,0,0,1,40,30+index*60],alpha:1,kind:{type:'rectangle',width:120,height:40,fill:[40,80,160,255],stroke:[0,0,0,0],stroke_width:0}})) : []),
    scripts_json: () => '[]',
    accessibility_json: () => JSON.stringify(runtimeAccessibility()),
    activate_accessible: () => {activations++;return false;},
    is_dispatch_current: () => true,
    script_overflowed: () => false,
    lifetime_id: key => lifetimes.get(key) || '',
    child_path: () => 'null',
    visible: () => true,
    property: () => 0,
  };

  const sandbox = {
    console: { warn() {}, info() {}, error() {} },
    location: { protocol: 'file:', href: 'file:///Users/miles/Projects/NeoFlash/examples/test/neoflash-export/index.html' },
    atob: b64 => Buffer.from(b64, 'base64').toString('binary'),
    Uint8Array,
    Map,
    WeakMap,
    Set,
    Promise,
    Error,
    TypeError,
    Math,
    Number,
    String,
    Boolean,
    Array,
    Object,
    JSON,
    Symbol,
    Reflect,
    Path2D: class { moveTo() {} lineTo() {} closePath() {} },
    ResizeObserver: class { observe() {} },
    requestAnimationFrame(cb) { rafCallback = cb; },
    addEventListener() {},
    devicePixelRatio: 1,
    document: {
      body,
      visibilityState: 'visible',
      getElementById: id => elements.get(id) || makeElement(id),
      querySelector: sel => sel === 'footer' ? footer : sel === '.transport' ? transport : null,
      addEventListener() {},
    },
    NeoFlashWasmBase64: Buffer.from('\0asm\x01\0\0\0', 'binary').toString('base64'),
    NeoFlashWasm: Object.assign(
      () => Promise.resolve(),
      {
        WebPlayer: function() { return mockPlayer; },
      }
    ),
    NeoFlashBundle: {
      project: {
        format_version: 1,
        name: 'A11y Timeline Sync',
        width: 800,
        height: 600,
        fps: 10,
        background: [20, 24, 32, 255],
        root: 'root',
        symbols: [{
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 2,
            scripts: [],
            layers: [{
              id: 'layer-main',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  elements: [
                    {
                      id: 'el-f0-cta',
                      name: 'start_btn',
                      visible: true,
                      transform: { x: 40, y: 120, scale_x: 1, scale_y: 1, rotation: 0 },
                      kind: { type: 'rectangle', width: 120, height: 40, fill: [40, 80, 160, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
                      accessibility: {
                        name: 'Begin Quest',
                        description: 'Starts level 1',
                        role: 'button',
                        tab_order: 2,
                        disabled: false,
                      },
                    },
                    {
                      id: 'el-f0-title',
                      name: 'intro_heading',
                      visible: true,
                      transform: { x: 40, y: 30, scale_x: 1, scale_y: 1, rotation: 0 },
                      kind: { type: 'text', text: 'Welcome Hero', font_family: 'Inter', font_size: 24, color: [255, 255, 255, 255] },
                      accessibility: {
                        name: 'Welcome Hero Heading',
                        description: 'Main screen banner',
                        role: 'image',
                        tab_order: 1,
                        disabled: false,
                      },
                    },
                  ],
                  extended_accessibility: [
                    {
                      element_id: 'el-f0-title',
                      role: 'heading',
                      live: 'polite',
                      shortcut: '',
                      silent: false,
                    },
                    {
                      element_id: 'el-f0-cta',
                      role: 'link',
                      live: 'off',
                      shortcut: 'Enter',
                      silent: false,
                    },
                  ],
                },
                {
                  frame: 1,
                  elements: [
                    {
                      id: 'el-f1-dialog',
                      name: 'victory_modal',
                      visible: true,
                      transform: { x: 100, y: 80, scale_x: 1, scale_y: 1, rotation: 0 },
                      kind: { type: 'rectangle', width: 320, height: 180, fill: [30, 50, 70, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
                      accessibility: {
                        name: 'Mission Complete',
                        description: 'All objectives cleared',
                        role: 'image',
                        tab_order: 1,
                        disabled: false,
                      },
                    },
                  ],
                  extended_accessibility: [
                    {
                      element_id: 'el-f1-dialog',
                      role: 'dialog',
                      live: 'assertive',
                      shortcut: 'Escape',
                      silent: false,
                    },
                  ],
                },
              ],
            }],
          },
        }],
      },
      scripts: [],
      wasm: Buffer.from('\0asm\x01\0\0\0', 'binary').toString('base64'),
      publish: { level: 'full', omitted_modules: [] },
      warnings: [],
    },
  };
  sandbox.globalThis = sandbox;
  sandbox.window = sandbox;
  const ctx = vm.createContext(sandbox);
  for (const src of [pathJs, accessibilityJs, previewJs, eventsJs, stageJs, displayJs, runtimeJs]) {
    vm.runInContext(src, ctx);
  }
  await new Promise(resolve => setTimeout(resolve, 10));

  assert.equal(elements.get('error').style.display, 'none', `unexpected boot error: ${elements.get('error').textContent}`);
  assert.equal(overlayEl.getAttribute('role'), 'region');
  const semanticEl=elements.get('semantic-layer');
  assert.equal(overlayEl.children.length, 0, 'runtime-covered targets have no second inert overlay node');
  assert.equal(semanticEl.children.length, 2);
  assert.equal(semanticEl.children[0].getAttribute('role'), 'heading');
  assert.equal(semanticEl.children[0].getAttribute('aria-label'), 'Welcome Hero Heading');
  assert.equal(semanticEl.children[0].getAttribute('aria-description'), 'Main screen banner');
  assert.equal(semanticEl.children[0].getAttribute('aria-live'), 'polite');
  assert.equal(semanticEl.children[0].getAttribute('tabindex'), '1');

  assert.equal(semanticEl.children[1].tagName, 'BUTTON');
  assert.equal(semanticEl.children[1].getAttribute('role'), 'link');
  assert.equal(semanticEl.children[1].getAttribute('aria-label'), 'Begin Quest');
  assert.equal(semanticEl.children[1].getAttribute('aria-keyshortcuts'), 'Enter');
  assert.equal(semanticEl.children[1].getAttribute('tabindex'), '2');

  const stableTitleNode = semanticEl.children[0];
  const stableButtonNode = semanticEl.children[1];
  stableButtonNode.focus();stableButtonNode.listeners.click();assert.equal(activations,1);
  assert.equal(typeof rafCallback, 'function');
  rafCallback(1000);
  assert.equal(semanticEl.children.length, 2);
  assert.equal(semanticEl.children[0], stableTitleNode, 'same authored/runtime lifetime preserves the DOM node');
  assert.equal(semanticEl.children[1], stableButtonNode);assert.equal(mockDocument.activeElement,stableButtonNode);

  // Re-instantiating one placement with the same authored name but a fresh lifetime retires only
  // that placement's DOM node; the sibling keeps its identity and focus state.
  lifetimes.set('["intro_heading"]', 'heading-life-2');
  rafCallback(1001);
  assert.equal(semanticEl.children.length, 2);
  assert.notEqual(semanticEl.children[0], stableTitleNode);
  assert.equal(semanticEl.children[1], stableButtonNode);

  // Advance runtime frame loop via requestAnimationFrame (10 fps => 100ms step).
  rafCallback(1120);

  assert.equal(activeFrame, 1, 'runtime advanced to frame 1');
  assert.equal(semanticEl.children.length, 1, 'frame 1 replaces the semantic targets');
  assert.equal(semanticEl.children[0].getAttribute('role'), 'dialog');
  assert.equal(semanticEl.children[0].getAttribute('aria-label'), 'Mission Complete');
  assert.equal(semanticEl.children[0].getAttribute('aria-description'), 'All objectives cleared');
  assert.equal(semanticEl.children[0].getAttribute('aria-live'), 'assertive');
  assert.equal(semanticEl.children[0].getAttribute('aria-keyshortcuts'), 'Escape');
  assert.equal(semanticEl.children[0].getAttribute('tabindex'), '1');
  stableButtonNode.listeners.click();assert.equal(activations,1,'the removed frame cannot activate through a retained DOM handler');
  sceneVisible=false;rafCallback(1121);
  assert.equal(semanticEl.children.length,0);assert.equal(overlayEl.children.length,0,'absent artwork cannot reappear through the authored fallback overlay');
});

test('AS2 showRedrawRegions renders and clears Canvas/DOM dirty outlines and ContextMenu builtInItems supports all 8 flags', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');

  const runtimeDir = path.resolve(__dirname, '../../target/web-runtime');
  const wasmCtx = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
  vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), wasmCtx);
  const wasm = vm.runInContext('NeoFlashWasm', wasmCtx);
  wasmCtx.wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
  await vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', wasmCtx);

  const NeoFlashGeom = require('../geom.js');
  const NeoFlashAS2Stage = require('../as2-stage.js');
  const geom = NeoFlashGeom.create(wasm, { getVector3D: () => null });

  // 1. Verify all 8 ContextMenuBuiltInItems properties + hideBuiltInItems + copy + builtin actions
  let loopFlag = true;
  let qualityVal = 'HIGH';
  let zoomCalls = [];
  const stageMod = NeoFlashAS2Stage.create({
    wasm,
    width: 400,
    height: 300,
    geometry: geom,
    legacy: { Stage: { width: 400, height: 300, addListener() {}, removeListener() {} }, broadcastStage() {} },
    display: {
      alive: () => true,
      descriptor: () => ({ owner: 'movie_clip' }),
      parentSpace: () => null,
      as2GlobalPropRef: name => name === '_quality' ? { get: () => qualityVal, set: v => { qualityVal = v; } } : null,
      toggleHighQuality: () => { qualityVal = qualityVal === 'HIGH' ? 'LOW' : 'HIGH'; },
    },
    getRoot: () => ({ play() {}, stop() {}, gotoAndStop() {}, prevFrame() {}, nextFrame() {} }),
    getLoop: () => loopFlag,
    toggleLoop: () => { loopFlag = !loopFlag; return loopFlag; },
    onZoom: (dir, scale) => zoomCalls.push([dir, scale]),
  });
  const cm = new stageMod.ContextMenu();
  const allEight = ['save', 'zoom', 'quality', 'play', 'loop', 'rewind', 'forward_back', 'print'];
  for (const key of allEight) {
    assert.equal(cm.builtInItems[key], true, `default builtInItems.${key} should be true`);
    cm.builtInItems[key] = false;
    assert.equal(cm.builtInItems[key], false, `builtInItems.${key} should toggle to false without throwing`);
    cm.builtInItems[key] = true;
  }
  const cmCopy = cm.copy();
  cm.hideBuiltInItems();
  for (const key of allEight) {
    assert.equal(cm.builtInItems[key], false, `hideBuiltInItems clears ${key}`);
    assert.equal(cmCopy.builtInItems[key], true, `copied ContextMenu preserves ${key}`);
  }
  stageMod.triggerBuiltin('loop');
  assert.equal(stageMod.menuState().loop, false);
  stageMod.triggerBuiltin('quality');
  assert.equal(qualityVal, 'LOW');
  stageMod.triggerBuiltin('zoom_in');
  stageMod.triggerBuiltin('zoom_out');
  stageMod.triggerBuiltin('show_all');
  assert.deepEqual(zoomCalls, [['in', 2], ['out', 1], ['fit', 1]]);
  stageMod.dispose();

  // 2. Verify showRedrawRegions(true, 0xFF0000) -> Canvas strokeRect + DOM overlay -> showRedrawRegions(false)
  const project = {
    format_version: 2,
    name: 'Redraw Regions Test',
    width: 320,
    height: 240,
    fps: 10,
    background: [20, 24, 32, 255],
    root: 'root',
    symbols: [{
      id: 'root',
      name: 'Scene 1',
      kind: 'movie_clip',
      timeline: {
        frame_count: 3,
        scripts: [
          { frame: 0, script: { language: 'action_script2', source: 'showRedrawRegions(true, 0xFF0000);' } },
          { frame: 2, script: { language: 'action_script2', source: 'showRedrawRegions(false);' } },
        ],
        layers: [{
          id: 'layer-1',
          name: 'Layer 1',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [{
                id: 'box-1',
                name: 'box_mc',
                visible: true,
                transform: { x: 30, y: 40, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                kind: { type: 'rectangle', width: 80, height: 50, fill: [40, 120, 220, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
                scripts: [],
              }],
            },
            {
              frame: 1,
              label: '',
              tween: null,
              elements: [{
                id: 'box-1',
                name: 'box_mc',
                visible: true,
                transform: { x: 110, y: 60, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                kind: { type: 'rectangle', width: 80, height: 50, fill: [40, 120, 220, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
                scripts: [],
              }],
            },
            {
              frame: 2,
              label: '',
              tween: null,
              elements: [{
                id: 'box-1',
                name: 'box_mc',
                visible: true,
                transform: { x: 150, y: 70, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                kind: { type: 'rectangle', width: 80, height: 50, fill: [40, 120, 220, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
                scripts: [],
              }],
            },
          ],
        }],
      },
    }],
  };

  const realPlayer = new wasm.WebPlayer(JSON.stringify(project));
  const strokeRects = [];
  const mockDocument = { activeElement: null, createElement: tag => makeEl('', tag) };
  function makeEl(id = '', tag = 'div') {
    const attrs = new Map();
    const children = [];
    return {
      id,
      tagName: tag.toUpperCase(),
      parentNode: null,
      listeners: {},
      hidden: false,
      disabled: false,
      textContent: '',
      value: 'fit',
      clientWidth: 320,
      clientHeight: 240,
      children,
      ownerDocument: mockDocument,
      style: { display: 'none' },
      classList: { toggle() {} },
      setAttribute(k, v) { attrs.set(k, String(v)); },
      getAttribute(k) { return attrs.get(k) ?? null; },
      removeAttribute(k) { attrs.delete(k); },
      querySelector(sel) {
        if (sel === '[data-neoflash-redraw-regions]') {
          return children.find(c => c.getAttribute('data-neoflash-redraw-regions') !== null) || null;
        }
        return null;
      },
      addEventListener(name, h) { this.listeners[name] = h; },
      removeEventListener() {},
      replaceChildren(...next) {
        for (const c of children.slice()) c.remove();
        for (const c of next) this.appendChild(c);
      },
      appendChild(c) { c.remove?.(); children.push(c); c.parentNode = this; return c; },
      insertBefore(c, before) { c.remove?.(); children.splice(before ? children.indexOf(before) : children.length, 0, c); c.parentNode = this; },
      removeChild(c) { c.remove?.(); },
      remove() { if (this.parentNode) { const s = this.parentNode.children; s.splice(s.indexOf(this), 1); this.parentNode = null; } },
      getBoundingClientRect() { return { left: 0, top: 0, width: 320, height: 240 }; },
      getContext() {
        return {
          strokeStyle: '#000',
          lineWidth: 1,
          setTransform() {},
          clearRect() {},
          fillRect() {},
          save() {},
          restore() {},
          transform() {},
          beginPath() {},
          rect() {},
          arc() {},
          fill() {},
          stroke() {},
          strokeRect(x, y, w, h) { strokeRects.push({ style: this.strokeStyle, x, y, w, h }); },
          fillText() {},
          measureText(str) { return { width: String(str).length * 8 }; },
        };
      },
    };
  }

  const stageSurfaceEl = makeEl('stage-surface');
  const elements = new Map([
    ['stage', makeEl('stage', 'canvas')],
    ['stage-space', makeEl('stage-space')],
    ['stage-surface', stageSurfaceEl],
    ['semantic-layer', makeEl('semantic-layer')],
    ['stage-a11y-overlay', makeEl('stage-a11y-overlay')],
    ['viewport', makeEl('viewport')],
    ['status', makeEl('status')],
    ['toggle', makeEl('toggle')],
    ['restart', makeEl('restart')],
    ['zoom', makeEl('zoom')],
    ['pan', makeEl('pan')],
    ['audio-toggle', makeEl('audio-toggle')],
    ['report-toggle', makeEl('report-toggle')],
    ['name', makeEl('name')],
    ['load-info', makeEl('load-info')],
    ['audio-status', makeEl('audio-status')],
    ['trace', makeEl('trace')],
    ['size-report', makeEl('size-report')],
    ['error', makeEl('error')],
    ['loading', makeEl('loading')],
  ]);
  const footer = makeEl('footer');
  const transport = makeEl('transport');
  const body = makeEl('body');
  let rafCb = null;

  const sandbox = {
    console: { warn() {}, info() {}, error() {} },
    location: { protocol: 'file:', href: 'file:///tmp/redraw-test/index.html' },
    atob: b64 => Buffer.from(b64, 'base64').toString('binary'),
    Uint8Array, Map, WeakMap, Set, Promise, Error, TypeError, RangeError, Math, Number, String, Boolean, Array, Object, JSON, Symbol, Reflect,
    Path2D: class { moveTo() {} lineTo() {} quadraticCurveTo() {} bezierCurveTo() {} closePath() {} rect() {} },
    ResizeObserver: class { observe() {} },
    requestAnimationFrame(cb) { rafCb = cb; },
    addEventListener() {},
    devicePixelRatio: 1,
    document: {
      body,
      visibilityState: 'visible',
      createElement: tag => makeEl('', tag),
      getElementById: id => elements.get(id) || makeEl(id),
      querySelector: sel => sel === 'footer' ? footer : sel === '.transport' ? transport : null,
      addEventListener() {},
    },
    NeoFlashWasmBase64: Buffer.from('\0asm\x01\0\0\0', 'binary').toString('base64'),
    NeoFlashWasm: Object.assign(() => Promise.resolve(wasm), wasm, {
      WebPlayer: function() { return realPlayer; },
    }),
    NeoFlashBundle: {
      project,
      scripts: [
        {
          frame: 0,
          language: 'action_script2',
          source: 'showRedrawRegions(true, 0xFF0000);',
          code: '__nfLegacy.showRedrawRegions(true, 0xFF0000);',
        },
        {
          frame: 2,
          language: 'action_script2',
          source: 'showRedrawRegions(false);',
          code: '__nfLegacy.showRedrawRegions(false);',
        },
      ],
      wasm: Buffer.from('\0asm\x01\0\0\0', 'binary').toString('base64'),
      publish: { level: 'full', omitted_modules: [] },
      warnings: [],
    },
  };
  sandbox.globalThis = sandbox;
  sandbox.window = sandbox;
  const ctx = vm.createContext(sandbox);
  for (const rel of ['../path.js', '../geom.js', '../geom3d.js', '../accessibility.js', '../preview.js', '../events.js', '../stage.js', '../display.js', '../runtime.js']) {
    vm.runInContext(fs.readFileSync(path.resolve(__dirname, rel), 'utf8'), ctx);
  }
  await new Promise(resolve => setTimeout(resolve, 15));

  try {
    assert.equal(elements.get('error').style.display, 'none', `unexpected boot error: ${elements.get('error').textContent}`);
    assert.ok(strokeRects.some(r => r.style === '#ff0000' && r.x === 30 && r.y === 40 && r.w === 80 && r.h === 50),
      `expected Canvas strokeRect('#ff0000', 30, 40, 80, 50), got ${JSON.stringify(strokeRects)}`);
    const domLayer = stageSurfaceEl.querySelector('[data-neoflash-redraw-regions]');
    assert.ok(domLayer && domLayer.children.length > 0, 'DOM redraw-regions overlay created with active box');
    assert.equal(domLayer.children[0].style.border, '1px solid #ff0000');

    // Frame 1: box_mc moves from (30,40) to (110,60) -> both old and new dirty regions outlined!
    strokeRects.length = 0;
    rafCb(1000);
    rafCb(1120);
    assert.ok(strokeRects.some(r => r.x === 30 && r.y === 40), 'previous position is outlined as dirty');
    assert.ok(strokeRects.some(r => r.x === 110 && r.y === 60), 'new position is outlined as dirty');

    // Frame 2: showRedrawRegions(false) clears both Canvas strokeRects and DOM overlay!
    strokeRects.length = 0;
    rafCb(1240);
    assert.equal(strokeRects.length, 0, 'showRedrawRegions(false) stops drawing Canvas dirty outlines');
    assert.equal(domLayer.children.length, 0, 'showRedrawRegions(false) clears DOM redraw-region boxes');
  } finally {
    realPlayer.free();
  }
});

test('Preview.resize and layout prevent HiDPI classic-scrollbar ResizeObserver oscillation across Fit, 100% and 200% zoom', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const { layout, viewportBox, Preview } = require('../preview.js');

  // 1. IEEE-754 float overshoot guard: 600 * (529 / 600) === 529.0000000000001 in float64,
  //    which previously produced spaceHeight = 561.0000000000001 > viewportHeight (561).
  const ieeeFit = layout(960, 600, 1000, 561, 'fit', 16);
  assert.ok(ieeeFit.stageHeight <= 529, `expected stageHeight <= 529, got ${ieeeFit.stageHeight}`);
  assert.equal(ieeeFit.spaceWidth, 1000);
  assert.equal(ieeeFit.spaceHeight, 561);

  // 2. Reproduce exact Ubuntu Retina XWayland Chrome geometry (`1018 x 647.50` CSS px at dpr=2,
  //    where DOM rounds offsetHeight/clientHeight to 648 while classic scrollbars take 15px
  //    whenever overflow !== 'hidden' and #stage-space exceeds the unrounded 1018.00 x 647.50 box).
  const space = { style: { width: '0px', height: '0px' } };
  const surface = { style: { left: '0px', top: '0px', width: '0px', height: '0px' } };
  const canvas = { width: 0, height: 0 };
  const trueW = 1018.0;
  const trueH = 647.5;
  const scrollbarPx = 15;

  function computeScrollbars() {
    if (viewport.style.overflow === 'hidden') return [0, 0];
    const spW = parseFloat(space.style.width) || 0;
    const spH = parseFloat(space.style.height) || 0;
    let vBar = spH > trueH ? scrollbarPx : 0;
    let hBar = spW > (trueW - vBar) ? scrollbarPx : 0;
    if (hBar && !vBar && spH > (trueH - hBar)) vBar = scrollbarPx;
    return [vBar, hBar];
  }

  const viewport = {
    scrollLeft: 0,
    scrollTop: 0,
    style: { overflow: '' },
    classList: { toggle() {} },
    addEventListener() {},
    hasPointerCapture: () => false,
    releasePointerCapture() {},
    get offsetWidth() { return Math.round(trueW); },
    get offsetHeight() { return Math.round(trueH); },
    get clientWidth() {
      const [vBar] = computeScrollbars();
      return Math.round(trueW - vBar);
    },
    get clientHeight() {
      const [, hBar] = computeScrollbars();
      return Math.round(trueH - hBar);
    },
    getBoundingClientRect() {
      return { left: 0, top: 0, width: trueW, height: trueH };
    },
  };

  assert.deepEqual(viewportBox(viewport), [1018, 647], 'subpixel 647.50 height floors to 647 rather than rounding up to 648');

  const preview = new Preview(viewport, space, surface, canvas, { width: 960, height: 600 }, () => {});
  const seenStates = new Set();
  for (let frame = 0; frame < 20; frame++) {
    preview.resize();
    seenStates.add(`${viewport.clientWidth}x${viewport.clientHeight}:${space.style.width}x${space.style.height}:${surface.style.left},${surface.style.top}`);
  }
  assert.equal(viewport.style.overflow, 'hidden', 'Fit mode disables viewport scrollbars');
  assert.equal(seenStates.size, 1, `Fit mode must be 100% stable across ResizeObserver frames, got ${[...seenStates].join(' | ')}`);
  assert.deepEqual(computeScrollbars(), [0, 0]);

  // 3. Switching to 100% ('1') and 50% ('0.5') enables overflow:auto for scrollable zoom modes,
  //    yet still produces zero scrollbar oscillation when 960x600 + 32px gutter fits inside 1018x647.5.
  preview.setMode('1');
  assert.equal(viewport.style.overflow, 'auto');
  const seen100 = new Set();
  for (let frame = 0; frame < 10; frame++) {
    preview.resize();
    seen100.add(`${viewport.clientWidth}x${viewport.clientHeight}:${space.style.width}x${space.style.height}`);
  }
  assert.equal(seen100.size, 1, `100% mode must not oscillate on subpixel HiDPI height, got ${[...seen100].join(' | ')}`);
  assert.deepEqual(computeScrollbars(), [0, 0], 'no classic scrollbars when 960x600 stage fits inside 1018x647.5');

  // 4. Switching to 200% ('2') enables scrolling over 1952x1232, and returning to 'fit' immediately clears scrollbars.
  preview.setMode('2');
  assert.equal(viewport.style.overflow, 'auto');
  assert.equal(space.style.width, '1952px');
  assert.equal(space.style.height, '1232px');
  assert.deepEqual(computeScrollbars(), [15, 15], 'zoomed 200% stage exposes classic scrollbars for panning');
  viewport.scrollLeft = 240;
  viewport.scrollTop = 180;

  preview.setMode('fit');
  assert.equal(viewport.style.overflow, 'hidden');
  assert.equal(viewport.clientWidth, 1018, 'returning to Fit immediately reclaims the 15px scrollbar gutter');
  assert.equal(viewport.clientHeight, 648);
  assert.equal(space.style.width, '1018px');
  assert.equal(space.style.height, '647px');
  assert.equal(viewport.scrollLeft, 0);
  assert.equal(viewport.scrollTop, 0);

  // 5. Scoped module CSS keeps #viewport overflow:hidden by default and clamps #trace to a stable single line.
  const runtimeSrc = fs.readFileSync(path.resolve(__dirname, '../runtime.js'), 'utf8');
  assert.match(runtimeSrc, /\[data-neoflash-role="viewport"\][^}]*overflow:\s*hidden/);
  assert.match(runtimeSrc, /\[data-neoflash-role="trace"\][^}]*white-space:\s*nowrap/);
});
