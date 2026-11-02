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
