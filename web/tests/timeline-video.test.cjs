const assert = require('node:assert/strict');
const test = require('node:test');

const NeoFlashEvents = require('../events.js');
const NeoFlashVideo = require('../video.js');

test('authored timeline video binds full placement paths, decodes bundled bytes and owns transport lifecycle', async () => {
  const targetRef = { generation: 1, id: 7 };
  const targetKey = JSON.stringify(targetRef);
  const player = {
    object_ref(path) {
      assert.deepEqual(JSON.parse(path), ['outer', 'video']);
      return JSON.stringify(targetRef);
    },
    object_alive(key) { return key === targetKey; },
    object_video_info(key) {
      assert.equal(key, targetKey);
      return JSON.stringify({ width: 320, height: 180, smoothing: false, deblocking: 0 });
    },
  };
  const bytes = Uint8Array.from(Buffer.from('\0\0\0\x18ftypisom\0\0\x02\0isomiso2avc1mp41', 'binary'));
  const net = {
    mediaSource(path) {
      assert.equal(path, 'media/imported.mp4');
      return { kind: 'resource', bytes: bytes.slice(), mediaType: 'video/mp4', length: bytes.length };
    },
  };
  const proxies = new Map();
  const references = new WeakMap();
  const proxyForRef = ref => {
    const key = JSON.stringify(ref);
    if (!proxies.has(key)) {
      const target = {};
      proxies.set(key, target);
      references.set(target, { key, objectRef: ref });
    }
    return proxies.get(key);
  };

  const media = [];
  const blobs = [];
  const revoked = [];
  let gesture = false;
  function createVideo() {
    const listeners = new Map();
    const element = {
      muted: false,
      volume: 1,
      paused: true,
      readyState: 0,
      videoWidth: 0,
      videoHeight: 0,
      duration: NaN,
      currentTime: 0,
      src: '',
      plays: 0,
      addEventListener(type, fn) {
        if (!listeners.has(type)) listeners.set(type, []);
        listeners.get(type).push(fn);
      },
      removeEventListener(type, fn) {
        const list = listeners.get(type) || [];
        const index = list.indexOf(fn);
        if (index >= 0) list.splice(index, 1);
      },
      dispatch(type) {
        for (const fn of [...(listeners.get(type) || [])]) fn();
      },
      setAttribute(name, value) { this[name] = value; },
      removeAttribute(name) { this[name] = ''; },
      load() {
        if (!this.src) return;
        assert.equal(this.muted, true, 'source is browser-muted before decoding');
        assert.equal(this.volume, 0, 'source starts with zero audible gain');
        this.videoWidth = 1080;
        this.videoHeight = 1920;
        this.duration = 4;
        this.readyState = 1;
        this.dispatch('loadedmetadata');
        this.readyState = 2;
        this.dispatch('loadeddata');
      },
      play() {
        this.plays += 1;
        this.mutedAtPlay = this.muted;
        this.paused = false;
        this.dispatch('playing');
        return Promise.resolve();
      },
      pause() {
        const changed = !this.paused;
        this.paused = true;
        if (changed) this.dispatch('pause');
      },
    };
    media.push(element);
    return element;
  }
  const createCanvas = (width, height) => ({
    width,
    height,
    getContext: () => ({ drawImage() {} }),
  });
  const video = NeoFlashVideo.create({
    player,
    net,
    events: NeoFlashEvents,
    referenceOf: target => references.get(target),
    proxyForRef,
    resourcePath: id => id === 'movie' ? 'media/imported.mp4' : null,
    createVideo,
    createCanvas,
    createObjectURL(blob) {
      blobs.push(blob);
      return `blob:timeline-${blobs.length}`;
    },
    revokeObjectURL(url) { revoked.push(url); },
    isUserGesture: () => gesture,
  });

  const stream = {
    op: 'seek_and_present',
    channel: 'nested-video',
    target_path: ['outer', 'video'],
    layer_id: 'video-layer',
    resource: 'movie',
    target_time_ms: 0,
    playing: true,
    muted: false,
    loop_video: false,
  };
  video.timeline([stream]);
  assert.equal(blobs.length, 1);
  assert.equal(blobs[0].size, bytes.length, 'bundled MP4 bytes reach the Blob decoder boundary');
  assert.equal(media[0].plays, 1, 'authored visuals advance before browser activation');
  assert.equal(media[0].mutedAtPlay, true, 'pre-activation playback remains browser-muted');
  assert.equal(video.status().audible, true);
  assert.equal(video.status().requiresGesture, true);
  assert.equal(video.status().state, 'awaiting_gesture', 'an authored audible span requests browser activation even while its visuals advance');

  video.timeline([{ ...stream, muted: true }]);
  assert.equal(video.status().audible, false);
  assert.equal(video.status().requiresGesture, false);
  assert.equal(video.status().state, 'running', 'an authored-muted span does not advertise a needless audio gesture');
  video.timeline([stream]);

  video.timeline([{ ...stream, target_time_ms: 1250 }]);
  assert.equal(media[0].currentTime, 1.25);
  video.timeline([{ op: 'pause_streams' }]);
  assert.equal(media[0].paused, true);

  gesture = true;
  await video.resumeFromGesture();
  assert.equal(media[0].muted, false, 'activation restores authored audio on an unmuted span');
  video.timeline([{ ...stream, target_time_ms: 2000, playing: false }]);
  assert.equal(media[0].currentTime, 2);
  assert.equal(media[0].paused, true, 'scrub presents a frame without restarting transport');

  const snapshot = video.prepareSnapshot({
    draws: [{ owner_ref: targetRef, kind: { type: 'video_frame', width: 320, height: 180 } }],
  });
  assert.deepEqual(snapshot.draws[0].kind.alpha_bounds, { left: 0, top: 0, right: 320, bottom: 180 });

  video.timeline([{ op: 'stop_channel', channel: 'nested-video' }]);
  assert.equal(revoked.length, 1, 'leaving the span releases its Blob URL');
  video.timeline([{ ...stream, op: 'start_event', channel: 'event-video', start_offset_ms: 500 }]);
  assert.equal(media.length, 2);
  assert.equal(media[1].currentTime, 0.5);
  video.timeline([{ op: 'stop', resource: 'movie' }]);
  assert.equal(revoked.length, 2, 'Stop sync retires the matching authored decoder');

  video.timeline([stream]);
  assert.equal(media.length, 3);
  video.timeline([{ op: 'stop_all' }]);
  assert.equal(revoked.length, 3, 'Restart/reset stop all timeline-owned decoders');
  video.reset();
  video.dispose();
});

test('script NetStream uses the canonical HTTP URL and can switch back to bundled media', () => {
  const bytes = Uint8Array.from(Buffer.from('\0\0\0\x18ftypisom', 'binary'));
  const requested = [];
  const net = {
    mediaSource(path) {
      requested.push(path);
      if (path === '/video-fixture.mp4') {
        return { kind: 'http', url: 'https://media.example.test/video-fixture.mp4' };
      }
      assert.equal(path, 'media/embedded.mp4');
      return { kind: 'resource', bytes: bytes.slice(), mediaType: 'video/mp4', length: bytes.length };
    },
  };
  const media = [];
  const failures = [];
  const revoked = [];
  function createVideo() {
    const listeners = new Map();
    const element = {
      muted: false,
      volume: 1,
      paused: true,
      readyState: 0,
      videoWidth: 320,
      videoHeight: 180,
      duration: 4,
      currentTime: 0,
      src: '',
      addEventListener(type, fn) {
        if (!listeners.has(type)) listeners.set(type, []);
        listeners.get(type).push(fn);
      },
      removeEventListener(type, fn) {
        const list = listeners.get(type) || [];
        const index = list.indexOf(fn);
        if (index >= 0) list.splice(index, 1);
      },
      emit(type) { for (const fn of [...(listeners.get(type) || [])]) fn(); },
      callbacks(type) { return [...(listeners.get(type) || [])]; },
      setAttribute(name, value) { this[name] = value; },
      removeAttribute(name) { this[name] = ''; },
      load() {},
      play() { this.paused = false; return Promise.resolve(); },
      pause() {
        this.paused = true;
        if (this.pauseFailure) {
          const failure = this.pauseFailure;
          this.pauseFailure = null;
          throw failure;
        }
      },
    };
    media.push(element);
    return element;
  }
  const video = NeoFlashVideo.create({
    player: { object_video_info() {} },
    net,
    events: NeoFlashEvents,
    createVideo,
    createObjectURL: () => 'blob:embedded-video',
    revokeObjectURL(url) { revoked.push(url); },
    isUserGesture: () => true,
    onError(error) { failures.push(error); throw error; },
  });
  let connection = new video.NetConnection();
  connection.connect(null);
  let stream = new video.NetStream(connection);

  stream.play('/video-fixture.mp4');
  assert.equal(media[0].src, 'https://media.example.test/video-fixture.mp4');

  stream.play('media/embedded.mp4');
  assert.equal(media[1].src, 'blob:embedded-video');
  assert.deepEqual(requested, ['/video-fixture.mp4', 'media/embedded.mp4']);

  for (const cleanupThrows of [false, true]) {
    const original = new Error('authored metadata callback failed');
    const secondary = new Error('browser video cleanup failed');
    failures.length = 0;
    stream.client = { onMetaData() { throw original; } };
    if (cleanupThrows) media.at(-1).pauseFailure = secondary;
    assert.throws(() => media.at(-1).emit('loadedmetadata'), error => error === original);
    assert.deepEqual(failures, cleanupThrows ? [original, secondary] : [original]);
    assert.equal(video.status().error, original.message);

    let metadata = null;
    stream.client = { onMetaData(value) { metadata = value; } };
    stream.play('/video-fixture.mp4');
    media.at(-1).emit('loadedmetadata');
    assert.equal(metadata.width, 320, 'the stream can load again after callback and cleanup failures');
  }

  for (const cleanup of ['reset', 'dispose']) {
    const other = new video.NetStream(connection);
    let staleMetadata = 0;
    stream.client = other.client = { onMetaData() { staleMetadata++; } };
    stream.play('media/embedded.mp4');
    other.play('media/embedded.mp4');
    const oldMedia = media.slice(-2), callbacks = oldMedia.map(element => element.callbacks('loadedmetadata')[0]);
    for (const element of oldMedia) element.emit('playing');
    const beforeRevoke = revoked.length;
    const primary = new Error(`first media ${cleanup} failed`), secondary = new Error(`second media ${cleanup} failed`);
    oldMedia[0].pauseFailure = primary;
    if (cleanup === 'dispose') oldMedia[1].pauseFailure = secondary;
    assert.throws(() => video[cleanup](), error => {
      if (cleanup === 'reset') return error === primary;
      assert.equal(error.cause, primary);
      assert.deepEqual(error.errors, [primary, secondary]);
      return true;
    });
    assert.deepEqual(oldMedia.map(element => element.paused), [true, true]);
    assert.deepEqual(oldMedia.map(element => element.src), ['', ''], 'both browser elements relinquish their sources');
    assert.equal(revoked.length - beforeRevoke, 2, 'every owned Blob URL is released despite pause failure');
    assert.deepEqual(oldMedia.map(element => element.callbacks('loadedmetadata').length), [0, 0]);
    for (const callback of callbacks) callback();
    assert.equal(staleMetadata, 0, 'queued browser callbacks cannot enter the retired generation');
    assert.throws(() => stream.play('/video-fixture.mp4'), /previous|disposed/);
    if (cleanup === 'reset') {
      connection = new video.NetConnection();
      connection.connect(null);
      stream = new video.NetStream(connection);
      stream.client = { onMetaData() { staleMetadata++; } };
      stream.play('/video-fixture.mp4');
      media.at(-1).emit('loadedmetadata');
      assert.equal(staleMetadata, 1, 'new playback works after a failed reset');
    }
  }
  assert.doesNotThrow(() => video.dispose(), 'failed disposal still retires the service exactly once');
  assert.throws(() => new video.NetConnection(), /disposed/);
});
