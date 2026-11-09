const assert = require('node:assert/strict');
const test = require('node:test');
const {localPoint, hitShape, oneBasedFrame, sourceKey} = require('../runtime.js');

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
  // Minimal valid 16-bit mono 8 kHz WAV header + 16 samples so format inspection passes.
  const wav = Buffer.alloc(44 + 32);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(36 + 32, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(8000, 24);
  wav.writeUInt32LE(16000, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(32, 40);

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
