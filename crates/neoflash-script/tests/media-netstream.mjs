// Comprehensive script-controlled file media playback verification (T-327):
// Non-RTMP NetConnection(null) + NetStream.play(url, start?, len?, reset?) across AS3 & AS2,
// progressive HTTP 206 Range + 302 redirects + data: URIs, container onCuePoint + onMetaData +
// onPlayStatus + Buffer.Full/Empty + Seek.Notify/InvalidTime, SoundTransform StereoPannerNode &
// 2x2 ChannelSplitter/Gain/ChannelMerger matrix across SoundChannel / AS2 Sound(mc) / SoundMixer /
// NetStream / Sprite hierarchy, Sound.isURLInaccessible CORS enforcement, and Pause/Close/Restart cleanup.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
import { homedir } from 'node:os';
import { readFileSync, writeFileSync, mkdtempSync, rmSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { createServer } from 'node:http';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { createRuntimeHost } from './host-integration.mjs';

async function until(predicate, label, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate() && Date.now() < deadline) {
    await new Promise((done) => setTimeout(done, 5));
  }
  assert.ok(predicate(), label);
}

function buildCueMarkedStereoWav({ sampleRate = 8000, durationSec = 0.5, cueTimeSec = 0.15, cueLabel = 'beat_1' } = {}) {
  const numFrames = Math.max(1, Math.round(sampleRate * durationSec));
  const channels = 2;
  const bytesPerSample = 2;
  const blockAlign = channels * bytesPerSample;
  const byteRate = sampleRate * blockAlign;
  const dataSize = numFrames * blockAlign;

  // fmt (24) + data (8 + dataSize) + cue (8 + 4 + 24) + LIST/adtl/labl
  const labelRaw = Buffer.from(`${cueLabel}\0`, 'utf8');
  const lablPayloadLen = 4 + labelRaw.length;
  const lablPadded = lablPayloadLen + (lablPayloadLen & 1);
  const listPayloadLen = 4 + 8 + lablPadded; // 'adtl' + 'labl' header + payload
  const cuePayloadLen = 4 + 24;
  const riffPayloadLen = 4 + (8 + 16) + (8 + dataSize) + (8 + cuePayloadLen) + (8 + listPayloadLen);

  const buf = Buffer.alloc(8 + riffPayloadLen);
  let off = 0;
  buf.write('RIFF', off); off += 4;
  buf.writeUInt32LE(riffPayloadLen, off); off += 4;
  buf.write('WAVE', off); off += 4;

  // fmt chunk
  buf.write('fmt ', off); off += 4;
  buf.writeUInt32LE(16, off); off += 4;
  buf.writeUInt16LE(1, off); off += 2; // PCM
  buf.writeUInt16LE(channels, off); off += 2;
  buf.writeUInt32LE(sampleRate, off); off += 4;
  buf.writeUInt32LE(byteRate, off); off += 4;
  buf.writeUInt16LE(blockAlign, off); off += 2;
  buf.writeUInt16LE(16, off); off += 2;

  // data chunk
  buf.write('data', off); off += 4;
  buf.writeUInt32LE(dataSize, off); off += 4;
  for (let i = 0; i < numFrames; i++) {
    const t = i / sampleRate;
    const left = Math.round(Math.sin(2 * Math.PI * 440 * t) * 18000);
    const right = Math.round(Math.cos(2 * Math.PI * 660 * t) * 9000);
    buf.writeInt16LE(left, off);
    buf.writeInt16LE(right, off + 2);
    off += 4;
  }

  // cue chunk (1 cue point, id = 1)
  const cueSampleOffset = Math.min(numFrames - 1, Math.max(0, Math.round(cueTimeSec * sampleRate)));
  buf.write('cue ', off); off += 4;
  buf.writeUInt32LE(cuePayloadLen, off); off += 4;
  buf.writeUInt32LE(1, off); off += 4; // count = 1
  buf.writeUInt32LE(1, off); off += 4; // cue id = 1
  buf.writeUInt32LE(cueSampleOffset, off); off += 4; // position
  buf.write('data', off); off += 4; // fccChunk
  buf.writeUInt32LE(0, off); off += 4; // chunkStart
  buf.writeUInt32LE(0, off); off += 4; // blockStart
  buf.writeUInt32LE(cueSampleOffset, off); off += 4; // sampleOffset

  // LIST / adtl / labl chunk
  buf.write('LIST', off); off += 4;
  buf.writeUInt32LE(listPayloadLen, off); off += 4;
  buf.write('adtl', off); off += 4;
  buf.write('labl', off); off += 4;
  buf.writeUInt32LE(lablPayloadLen, off); off += 4;
  buf.writeUInt32LE(1, off); off += 4; // cue id = 1
  labelRaw.copy(buf, off);

  return buf;
}

function shiftMp4ChunkOffsets(buf, start, end, threshold, delta) {
  let p = start;
  while (p + 8 <= end) {
    const sz = buf.readUInt32BE(p);
    const typ = buf.subarray(p + 4, p + 8).toString('ascii');
    const len = sz === 0 ? end - p : sz;
    if (len < 8 || p + len > end) break;
    if (typ === 'trak' || typ === 'mdia' || typ === 'minf' || typ === 'stbl') {
      shiftMp4ChunkOffsets(buf, p + 8, p + len, threshold, delta);
    } else if (typ === 'stco' && len >= 16) {
      const count = buf.readUInt32BE(p + 12);
      for (let i = 0; i < count && p + 16 + (i + 1) * 4 <= p + len; i++) {
        const off = buf.readUInt32BE(p + 16 + i * 4);
        if (off >= threshold) buf.writeUInt32BE(off + delta, p + 16 + i * 4);
      }
    } else if (typ === 'co64' && len >= 16) {
      const count = buf.readUInt32BE(p + 12);
      for (let i = 0; i < count && p + 16 + (i + 1) * 8 <= p + len; i++) {
        const off = buf.readBigUInt64BE(p + 16 + i * 8);
        if (off >= BigInt(threshold)) buf.writeBigUInt64BE(off + BigInt(delta), p + 16 + i * 8);
      }
    }
    p += len;
  }
}

function injectMp4ChapterCue(mp4Bytes, cueTimeSec = 0.25, cueName = 'chapter_intro') {
  // Find 'moov' top-level box and append a 'chpl' chapter atom inside 'udta', shifting stco/co64 offsets.
  let pos = 0;
  while (pos + 8 <= mp4Bytes.length) {
    const sz = mp4Bytes.readUInt32BE(pos);
    const typ = mp4Bytes.subarray(pos + 4, pos + 8).toString('ascii');
    const boxLen = sz === 0 ? mp4Bytes.length - pos : sz;
    if (boxLen < 8 || pos + boxLen > mp4Bytes.length) break;
    if (typ === 'moov') {
      const nameBytes = Buffer.from(cueName, 'utf8');
      const chplPayload = Buffer.alloc(9 + 9 + nameBytes.length);
      chplPayload[0] = 1;
      chplPayload[8] = 1;
      const ts100ns = BigInt(Math.round(cueTimeSec * 10_000_000));
      chplPayload.writeBigUInt64BE(ts100ns, 9);
      chplPayload[17] = nameBytes.length;
      nameBytes.copy(chplPayload, 18);

      const chplBox = Buffer.alloc(8 + chplPayload.length);
      chplBox.writeUInt32BE(chplBox.length, 0);
      chplBox.write('chpl', 4);
      chplPayload.copy(chplBox, 8);

      const moovEnd = pos + boxLen;
      const beforeMoov = mp4Bytes.subarray(0, pos);
      const moovBody = Buffer.from(mp4Bytes.subarray(pos + 8, moovEnd));
      const afterMoov = mp4Bytes.subarray(moovEnd);

      let uPos = 0;
      let foundUdta = null;
      while (uPos + 8 <= moovBody.length) {
        const uSz = moovBody.readUInt32BE(uPos);
        const uTyp = moovBody.subarray(uPos + 4, uPos + 8).toString('ascii');
        const uLen = uSz === 0 ? moovBody.length - uPos : uSz;
        if (uLen < 8 || uPos + uLen > moovBody.length) break;
        if (uTyp === 'udta') {
          foundUdta = { uPos, uLen };
          break;
        }
        uPos += uLen;
      }

      if (foundUdta) {
        const delta = chplBox.length;
        shiftMp4ChunkOffsets(moovBody, 0, moovBody.length, moovEnd, delta);
        const newUdtaHdr = Buffer.alloc(8);
        newUdtaHdr.writeUInt32BE(foundUdta.uLen + delta, 0);
        newUdtaHdr.write('udta', 4);
        const newMoovHdr = Buffer.alloc(8);
        newMoovHdr.writeUInt32BE(boxLen + delta, 0);
        newMoovHdr.write('moov', 4);
        return Buffer.concat([
          beforeMoov,
          newMoovHdr,
          moovBody.subarray(0, foundUdta.uPos),
          newUdtaHdr,
          moovBody.subarray(foundUdta.uPos + 8, foundUdta.uPos + foundUdta.uLen),
          chplBox,
          moovBody.subarray(foundUdta.uPos + foundUdta.uLen),
          afterMoov,
        ]);
      }

      const udtaBox = Buffer.alloc(8 + chplBox.length);
      udtaBox.writeUInt32BE(udtaBox.length, 0);
      udtaBox.write('udta', 4);
      chplBox.copy(udtaBox, 8);

      const delta = udtaBox.length;
      shiftMp4ChunkOffsets(moovBody, 0, moovBody.length, moovEnd, delta);
      const newMoovHeader = Buffer.alloc(8);
      newMoovHeader.writeUInt32BE(8 + moovBody.length + udtaBox.length, 0);
      newMoovHeader.write('moov', 4);
      return Buffer.concat([beforeMoov, newMoovHeader, moovBody, udtaBox, afterMoov]);
    }
    pos += boxLen;
  }
  return mp4Bytes;
}

function createMediaHarness() {
  const videoElements = [];
  const audioElements = [];
  const blobs = new Map();
  const revoked = [];
  const draws = [];
  const loads = [];
  const audioContexts = [];
  let serial = 0;

  function createCanvas(width, height) {
    const surface = { width, height };
    let matrix = [1, 0, 0, 1, 0, 0];
    const stack = [];
    const pixelData = new Uint8ClampedArray(Math.max(4, (width || 1) * (height || 1) * 4));
    const context = {
      canvas: surface,
      globalAlpha: 1,
      globalCompositeOperation: 'source-over',
      imageSmoothingEnabled: false,
      save() { stack.push([matrix.slice(), this.globalAlpha]); },
      restore() { if (stack.length) [matrix, this.globalAlpha] = stack.pop(); },
      getTransform() { const [a, b, c, d, e, f] = matrix; return { a, b, c, d, e, f }; },
      setTransform(...args) { matrix = args.length === 1 ? [args[0].a, args[0].b, args[0].c, args[0].d, args[0].e, args[0].f] : args; },
      clearRect() {},
      fillRect() { pixelData[0] = 32; pixelData[1] = 180; pixelData[2] = 240; pixelData[3] = 255; },
      drawImage(source, ...args) {
        draws.push({ canvas: surface, source, args, time: source.currentTime ?? source.frame?.time ?? 0 });
        surface.frame = { source, time: source.currentTime ?? source.frame?.time ?? 0 };
        pixelData[0] = 44; pixelData[1] = 196; pixelData[2] = 120; pixelData[3] = 255;
      },
      createImageData(w, h) { return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4) }; },
      putImageData() {},
      getImageData(x, y, w, h) {
        const out = new Uint8ClampedArray(w * h * 4);
        for (let i = 0; i < out.length; i += 4) {
          out[i] = pixelData[0]; out[i + 1] = pixelData[1]; out[i + 2] = pixelData[2]; out[i + 3] = pixelData[3];
        }
        return { width: w, height: h, data: out };
      },
    };
    surface.getContext = () => context;
    return surface;
  }

  function createAudioContext() {
    const sources = [];
    const mediaElementSources = [];
    const gains = [];
    const panners = [];
    const splitters = [];
    const mergers = [];

    function makeNode(kind, extra = {}) {
      const connections = [];
      return {
        kind,
        connections,
        disconnected: 0,
        connect(dest, outIdx = 0, inIdx = 0) {
          connections.push({ dest, outIdx, inIdx });
          return dest;
        },
        disconnect() {
          this.disconnected++;
          connections.length = 0;
        },
        ...extra,
      };
    }

    const ctx = {
      state: 'running',
      sampleRate: 44100,
      currentTime: 0,
      destination: { kind: 'destination' },
      sources,
      mediaElementSources,
      gains,
      panners,
      splitters,
      mergers,
      resume() { this.state = 'running'; return Promise.resolve(); },
      close() { this.state = 'closed'; return Promise.resolve(); },
      decodeAudioData(buffer) {
        const bytes = Buffer.from(buffer);
        const isWav = bytes.length >= 44 && bytes.subarray(0, 4).toString('ascii') === 'RIFF';
        const sr = isWav ? bytes.readUInt32LE(24) || 8000 : 22050;
        const ch = isWav ? bytes.readUInt16LE(22) || 2 : 2;
        const dur = isWav ? Math.max(0.1, (bytes.length - 44) / Math.max(1, bytes.readUInt32LE(28))) : 0.7;
        const length = Math.max(1, Math.round(sr * dur));
        return Promise.resolve({ numberOfChannels: ch, sampleRate: sr, length, duration: dur });
      },
      createGain() {
        const g = makeNode('gain', { gain: { value: 1 }, channelCount: 2, channelCountMode: 'max', channelInterpretation: 'speakers' });
        gains.push(g);
        return g;
      },
      createStereoPanner() {
        const p = makeNode('panner', { pan: { value: 0 } });
        panners.push(p);
        return p;
      },
      createChannelSplitter(count = 2) {
        const s = makeNode('splitter', { count });
        splitters.push(s);
        return s;
      },
      createChannelMerger(count = 2) {
        const m = makeNode('merger', { count });
        mergers.push(m);
        return m;
      },
      createMediaElementSource(mediaEl) {
        const ms = makeNode('mediaElementSource', { mediaElement: mediaEl });
        mediaElementSources.push(ms);
        return ms;
      },
      createBufferSource() {
        const s = makeNode('bufferSource', {
          buffer: null,
          loop: false,
          loopStart: 0,
          loopEnd: 0,
          onended: null,
          started: false,
          ended: false,
          stoppedExplicitly: false,
          start(when = 0, offset = 0) {
            this.started = true;
            this.startedAt = Math.max(ctx.currentTime, when);
            this.offset = offset;
          },
          stop() {
            this.stoppedExplicitly = true;
            queueMicrotask(() => {
              if (!this.ended) {
                this.ended = true;
                this.onended?.();
              }
            });
          },
          finish() {
            if (!this.ended) {
              this.ended = true;
              this.onended?.();
            }
          },
        });
        sources.push(s);
        return s;
      },
      advance(seconds) {
        this.currentTime += seconds;
        for (const s of sources) {
          if (!s.started || s.ended || s.loop) continue;
          const end = s.startedAt + (s.buffer?.duration || 0) - (s.offset || 0);
          if (this.currentTime >= end) s.finish();
        }
      },
    };
    audioContexts.push(ctx);
    return ctx;
  }

  function createVideo() {
    class MockVideo extends EventTarget {
      constructor() {
        super();
        this.muted = false;
        this.volume = 1;
        this.paused = true;
        this.ended = false;
        this.readyState = 0;
        this.videoWidth = 0;
        this.videoHeight = 0;
        this.duration = NaN;
        this._time = 0;
        this._src = '';
        this.crossOrigin = null;
        this.preload = 'auto';
        this.playsInline = false;
        this.loop = false;
        this.loadGeneration = 0;
        this._fetchController = null;
        this.frames = new Map();
        this.listenersCount = 0;
        this.buffered = { length: 0, start: () => 0, end: () => 0 };
        this.textTracks = [];
      }
      _abortFetch() {
        if (this._fetchController) {
          try { this._fetchController.abort(); } catch (_) {}
          this._fetchController = null;
        }
      }
      addEventListener(type, fn, opts) {
        this.listenersCount++;
        super.addEventListener(type, fn, opts);
      }
      removeEventListener(type, fn, opts) {
        this.listenersCount = Math.max(0, this.listenersCount - 1);
        super.removeEventListener(type, fn, opts);
      }
      get src() { return this._src; }
      set src(value) {
        if (value !== this._src) this._abortFetch();
        if (value) {
          assert.equal(this.muted, true, 'media must be muted before assigning src');
          assert.equal(this.volume, 0, 'media volume must be 0 before assigning src');
        }
        this._src = value;
      }
      get currentTime() { return this._time; }
      set currentTime(value) {
        this._time = Math.max(0, Number(value) || 0);
        this.ended = false;
        this.dispatchEvent(new Event('seeking'));
        queueMicrotask(() => {
          if (this.readyState >= 1) {
            this.dispatchEvent(new Event('seeked'));
            this.frame();
          }
        });
      }
      removeAttribute(name) {
        if (name === 'src') {
          this._abortFetch();
          this._src = '';
        } else {
          delete this[name];
        }
      }
      load() {
        this._abortFetch();
        const gen = ++this.loadGeneration;
        const url = this._src;
        this.readyState = 0;
        if (!url) return;
        const ctrl = new AbortController();
        this._fetchController = ctrl;
        (async () => {
          let bytes;
          let contentType = '';
          try {
            if (blobs.has(url)) {
              const b = blobs.get(url);
              contentType = b.type || '';
              bytes = Buffer.from(await b.arrayBuffer());
            } else {
              const resp = await fetch(url, { signal: ctrl.signal });
              if (!resp.ok) throw Object.assign(new Error(`HTTP ${resp.status}`), { code: 2 });
              contentType = resp.headers.get('content-type') || '';
              bytes = Buffer.from(await resp.arrayBuffer());
            }
          } finally {
            if (this._fetchController === ctrl) this._fetchController = null;
          }
          if (ctrl.signal.aborted || gen !== this.loadGeneration) return;
          loads.push({ element: this, url, bytes, contentType });
          const isWav = (bytes.length >= 12 && bytes.subarray(0, 4).toString() === 'RIFF' && bytes.subarray(8, 12).toString() === 'WAVE') || contentType.startsWith('audio/');
          if (isWav) {
            this.videoWidth = 0;
            this.videoHeight = 0;
            const byteRate = bytes.length >= 32 ? bytes.readUInt32LE(28) : 32000;
            this.duration = byteRate > 0 ? Math.max(0.2, (bytes.length - 44) / byteRate) : 0.5;
          } else {
            this.videoWidth = 320;
            this.videoHeight = 180;
            this.duration = 4.0;
          }
          const initialBuffered = Math.min(this.duration, url.startsWith('blob:') ? this.duration : this.duration * 0.5);
          this.buffered = { length: 1, start: () => 0, end: () => this._bufferedEnd };
          this._bufferedEnd = initialBuffered;
          this.readyState = 1;
          this.dispatchEvent(new Event('loadedmetadata'));
          if (gen !== this.loadGeneration) return;
          this.readyState = 3;
          this.dispatchEvent(new Event('progress'));
          this.dispatchEvent(new Event('loadeddata'));
          this.dispatchEvent(new Event('canplay'));
          this.frame();
        })().catch((err) => {
          if (err?.name === 'AbortError' || ctrl.signal.aborted || gen !== this.loadGeneration) return;
          this.error = { code: err?.code || 2, message: String(err?.message || err) };
          this.dispatchEvent(new Event('error'));
        });
      }
      async play() {
        const gen = this.loadGeneration;
        if (this.readyState < 2) {
          await new Promise((resolveDone) => this.addEventListener('loadeddata', resolveDone, { once: true }));
        }
        if (gen !== this.loadGeneration) return;
        this.paused = false;
        this.ended = false;
        queueMicrotask(() => {
          if (gen !== this.loadGeneration || this.paused) return;
          this.dispatchEvent(new Event('play'));
          this.dispatchEvent(new Event('playing'));
          this.frame();
        });
      }
      pause() {
        if (!this._src || this.readyState === 0) this._abortFetch();
        const wasRunning = !this.paused;
        this.paused = true;
        if (wasRunning) this.dispatchEvent(new Event('pause'));
      }
      requestVideoFrameCallback(cb) {
        const id = ++serial;
        this.frames.set(id, cb);
        return id;
      }
      cancelVideoFrameCallback(id) {
        this.frames.delete(id);
      }
      frame(nowMs = performance.now()) {
        const pending = [...this.frames.values()];
        this.frames.clear();
        for (const cb of pending) {
          cb(nowMs, { mediaTime: this._time, presentedFrames: ++serial, width: this.videoWidth, height: this.videoHeight });
        }
      }
      advance(seconds, nowMs = performance.now()) {
        if (this.paused || !Number.isFinite(this.duration)) return;
        this._time = Math.min(this.duration, this._time + seconds);
        this._bufferedEnd = Math.min(this.duration, Math.max(this._bufferedEnd || 0, this._time + 0.5));
        this.dispatchEvent(new Event('progress'));
        this.dispatchEvent(new Event('timeupdate'));
        this.frame(nowMs);
        if (this._time >= this.duration - 1e-6 && !this.paused) {
          this._bufferedEnd = this.duration;
          this.paused = true;
          this.ended = true;
          this.dispatchEvent(new Event('ended'));
        }
      }
    }
    const el = new MockVideo();
    videoElements.push(el);
    return el;
  }

  function createOpaqueAudio() {
    class MockOpaqueAudio extends EventTarget {
      constructor() {
        super();
        this.muted = true;
        this.volume = 0;
        this.paused = true;
        this.ended = false;
        this.loop = false;
        this.preload = 'auto';
        this.duration = 0.5;
        this._time = 0;
        this._src = '';
        this.listenersCount = 0;
      }
      addEventListener(type, fn, opts) {
        this.listenersCount++;
        super.addEventListener(type, fn, opts);
      }
      removeEventListener(type, fn, opts) {
        this.listenersCount = Math.max(0, this.listenersCount - 1);
        super.removeEventListener(type, fn, opts);
      }
      get src() { return this._src; }
      set src(v) { this._src = v; }
      get currentTime() { return this._time; }
      set currentTime(v) { this._time = Math.max(0, Number(v) || 0); }
      removeAttribute(name) { if (name === 'src') this._src = ''; }
      load() {
        const url = this._src;
        if (!url) return;
        queueMicrotask(() => {
          if (this._src !== url) return;
          this.duration = 0.5;
          this.dispatchEvent(new Event('loadedmetadata'));
          this.dispatchEvent(new Event('canplaythrough'));
        });
      }
      play() {
        this.paused = false;
        this.ended = false;
        return Promise.resolve();
      }
      pause() {
        this.paused = true;
      }
      finish() {
        this.paused = true;
        this.ended = true;
        this.dispatchEvent(new Event('ended'));
      }
    }
    const el = new MockOpaqueAudio();
    audioElements.push(el);
    return el;
  }

  return {
    videoElements,
    audioElements,
    blobs,
    revoked,
    draws,
    loads,
    audioContexts,
    createCanvas,
    createAudioContext,
    createVideo,
    createOpaqueAudio,
    createObjectURL(blob) {
      const url = `blob:neoflash-t327-${++serial}`;
      blobs.set(url, blob);
      return url;
    },
    revokeObjectURL(url) {
      revoked.push(url);
      blobs.delete(url);
    },
  };
}

export async function verifyMediaNetStream(root, cargo) {
  const workDir = mkdtempSync(resolve(root, 'target/media-netstream-'));
  const exportDir = resolve(workDir, 'published');
  const harness = createMediaHarness();

  // Extract the real H.264 MP4 fixture from examples/video-playback.nflash and add an MP4 chapter cue point!
  const exampleProj = JSON.parse(readFileSync(resolve(root, 'examples/video-playback.nflash'), 'utf8'));
  const rawMotionRes = exampleProj.resources.find((r) => r.path === 'media/motion.mp4');
  assert.ok(rawMotionRes, 'examples/video-playback.nflash must contain media/motion.mp4');
  const rawMp4 = Buffer.from(rawMotionRes.data_base64, 'base64');
  const cuedMp4 = injectMp4ChapterCue(rawMp4, 0.25, 'chapter_intro');
  const cuedWav = buildCueMarkedStereoWav({ sampleRate: 8000, durationSec: 0.5, cueTimeSec: 0.15, cueLabel: 'beat_1' });

  let mainServer = null;
  let corsServer = null;
  let restrictedServer = null;
  let player = null;
  let host = null;

  const serverStats = {
    rangeRequests: [],
    redirects: 0,
    corsHits: 0,
    restrictedHits: 0,
    slowHits: 0,
    slowAborted: 0,
  };

  try {
    // 1. Start 3 local HTTP servers:
    //    - mainServer: serves 206 Partial Content Range video/audio & 302 redirects (same-origin)
    //    - corsServer: cross-origin WITH Access-Control-Allow-Origin: *
    //    - restrictedServer: cross-origin WITHOUT Access-Control-Allow-Origin (triggers Sound.isURLInaccessible)
    corsServer = createServer((req, res) => {
      serverStats.corsHits++;
      res.writeHead(200, {
        'Content-Type': 'audio/wav',
        'Content-Length': cuedWav.length,
        'Access-Control-Allow-Origin': '*',
      });
      res.end(cuedWav);
    });
    await new Promise((done, reject) => { corsServer.once('error', reject); corsServer.listen(0, '127.0.0.1', done); });
    const corsOrigin = `http://127.0.0.1:${corsServer.address().port}`;

    restrictedServer = createServer((req, res) => {
      serverStats.restrictedHits++;
      res.writeHead(200, {
        'Content-Type': 'audio/wav',
        'Content-Length': cuedWav.length,
        // Intentionally NO Access-Control-Allow-Origin header!
      });
      res.end(cuedWav);
    });
    await new Promise((done, reject) => { restrictedServer.once('error', reject); restrictedServer.listen(0, '127.0.0.1', done); });
    const restrictedOrigin = `http://127.0.0.1:${restrictedServer.address().port}`;

    mainServer = createServer((req, res) => {
      const urlPath = (req.url || '/').split('?')[0];
      if (urlPath === '/redirect-video.mp4') {
        serverStats.redirects++;
        res.writeHead(302, { Location: '/range-video.mp4' });
        res.end();
        return;
      }
      if (urlPath === '/redirect-cors-audio.wav') {
        serverStats.redirects++;
        res.writeHead(302, { Location: `${corsOrigin}/cors-tone.wav` });
        res.end();
        return;
      }
      if (urlPath === '/range-video.mp4') {
        const rangeHdr = req.headers.range;
        if (rangeHdr) {
          serverStats.rangeRequests.push(rangeHdr);
          const m = /^bytes=(\d+)-(\d*)$/i.exec(rangeHdr.trim());
          if (m) {
            const start = Math.min(cuedMp4.length - 1, Number(m[1]));
            const end = m[2] ? Math.min(cuedMp4.length - 1, Number(m[2])) : cuedMp4.length - 1;
            const slice = cuedMp4.subarray(start, end + 1);
            res.writeHead(206, {
              'Content-Type': 'video/mp4',
              'Accept-Ranges': 'bytes',
              'Content-Range': `bytes ${start}-${end}/${cuedMp4.length}`,
              'Content-Length': slice.length,
            });
            res.end(slice);
            return;
          }
        }
        res.writeHead(200, {
          'Content-Type': 'video/mp4',
          'Accept-Ranges': 'bytes',
          'Content-Length': cuedMp4.length,
        });
        res.end(cuedMp4);
        return;
      }
      if (urlPath === '/slow-video.mp4') {
        // Stays open until aborted so we verify AbortController cleanup on close()/Restart!
        serverStats.slowHits++;
        res.writeHead(200, { 'Content-Type': 'video/mp4', 'Content-Length': cuedMp4.length });
        res.write(cuedMp4.subarray(0, 64));
        let counted = false;
        const onAbort = () => {
          if (!counted && !res.writableEnded) {
            counted = true;
            serverStats.slowAborted++;
          }
          try { res.destroy(); } catch (_) {}
        };
        req.on('aborted', onAbort);
        req.on('close', onAbort);
        return;
      }
      res.writeHead(404);
      res.end('Not found');
    });
    await new Promise((done, reject) => { mainServer.once('error', reject); mainServer.listen(0, '127.0.0.1', done); });
    const mainOrigin = `http://127.0.0.1:${mainServer.address().port}`;
    const baseURL = `${mainOrigin}/index.html`;

    // 2. Build a real NeoFlash project containing BOTH AS3 (document class) and AS2 (frame script + authored Video & MovieClips)
    const project = {
      format_version: 9,
      name: 'T-327 Media NetStream Suite',
      width: 640,
      height: 360,
      fps: 24,
      background: [15, 23, 42, 255],
      root: 'root',
      document_class: 'app.MediaSuiteMain',
      resources: [
        {
          id: 'res-video',
          name: 'motion.mp4',
          path: 'media/motion.mp4',
          media_type: 'video/mp4',
          data_base64: cuedMp4.toString('base64'),
        },
        {
          id: 'res-audio',
          name: 'cued.wav',
          path: 'audio/cued.wav',
          media_type: 'audio/wav',
          data_base64: cuedWav.toString('base64'),
        },
      ],
      sources: [
        {
          id: 'doc-class',
          path: 'app/MediaSuiteMain.as',
          language: 'action_script3',
          source: `package app {
  import flash.display.MovieClip;
  import flash.display.Sprite;
  import flash.events.Event;
  import flash.events.NetStatusEvent;
  import flash.media.Sound;
  import flash.media.SoundChannel;
  import flash.media.SoundLoaderContext;
  import flash.media.SoundMixer;
  import flash.media.SoundTransform;
  import flash.media.Video;
  import flash.net.NetConnection;
  import flash.net.NetStream;
  import flash.net.NetStreamPlayOptions;
  import flash.net.URLRequest;
  import flash.utils.ByteArray;

  public dynamic class MediaSuiteMain extends MovieClip {
    public var parentHolder:Sprite;
    public var video:Video;
    public var nc:NetConnection;
    public var rtmpNc:NetConnection;
    public var ns:NetStream;
    public var statusLog:Array = [];
    public var rtmpStatusLog:Array = [];
    public var playStatusLog:Array = [];
    public var metaLog:Array = [];
    public var cueLog:Array = [];
    public var lastSeekDetails:Number = -1;
    public var ready:Boolean = false;

    public function MediaSuiteMain() {
      stop();
      parentHolder = new Sprite();
      parentHolder.name = "videoParent";
      addChild(parentHolder);
      video = new Video(320, 180);
      video.smoothing = true;
      parentHolder.addChild(video);

      rtmpNc = new NetConnection();
      rtmpNc.addEventListener(NetStatusEvent.NET_STATUS, function(ev:NetStatusEvent):void {
        rtmpStatusLog.push(ev.info.code + ":" + ev.info.level);
      });
      rtmpNc.connect("rtmp://live.example.test/app");

      nc = new NetConnection();
      nc.addEventListener(NetStatusEvent.NET_STATUS, onConnStatus);
      nc.connect(null);
    }

    private function onConnStatus(ev:NetStatusEvent):void {
      if (ev.info.code == "NetConnection.Connect.Success") {
        ns = new NetStream(nc);
        ns.client = {
          onMetaData: function(info:Object):void {
            metaLog.push({ width: info.width, height: info.height, duration: info.duration, framerate: info.framerate });
          },
          onPlayStatus: function(info:Object):void {
            playStatusLog.push(info.code);
          },
          onCuePoint: function(info:Object):void {
            cueLog.push(info.name + "@" + Math.round(info.time * 100) / 100);
          }
        };
        ns.addEventListener(NetStatusEvent.NET_STATUS, function(se:NetStatusEvent):void {
          statusLog.push(se.info.code);
          if (se.info.code == "NetStream.Seek.InvalidTime" && se.info.details !== undefined) {
            lastSeekDetails = Number(se.info.details);
          }
        });
        video.attachNetStream(ns);
        ready = true;
      }
    }

    public function clearLogs():void {
      statusLog = [];
      playStatusLog = [];
      metaLog = [];
      cueLog = [];
      lastSeekDetails = -1;
    }

    public function playArgs(url:String, start:Number, len:Number, reset:Object):void {
      ns.play(url, start, len, reset);
    }

    public function play2Stream(url:String):void {
      var opts:NetStreamPlayOptions = new NetStreamPlayOptions();
      opts.streamName = url;
      ns.play2(opts);
    }

    public function setStreamMatrix(ll:Number, lr:Number, rl:Number, rr:Number):void {
      var st:SoundTransform = new SoundTransform(1, 0);
      st.leftToLeft = ll;
      st.leftToRight = lr;
      st.rightToLeft = rl;
      st.rightToRight = rr;
      ns.soundTransform = st;
    }

    public function setParentVolumePan(vol:Number, pan:Number):void {
      parentHolder.soundTransform = new SoundTransform(vol, pan);
    }

    public function setReceiveAudio(enabled:Boolean):void {
      ns.receiveAudio(enabled);
    }

    public function setReceiveVideo(enabled:Boolean):void {
      ns.receiveVideo(enabled);
    }

    public function setReceiveVideoFPS(value:Number):void {
      ns.receiveVideoFPS(value);
    }
  }
}`,
        },
      ],
      symbols: [
        {
          id: 'mc-sound-target',
          name: 'SoundTargetClip',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [{ id: 'st-l', name: 'L', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
            scripts: [],
          },
        },
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [
              {
                id: 'layer-1',
                name: 'Main',
                visible: true,
                locked: false,
                keyframes: [
                  {
                    frame: 0,
                    label: '',
                    tween: null,
                    elements: [
                      {
                        id: 'as2-video-el',
                        name: 'as2Video',
                        transform: { x: 320, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'video', width: 320, height: 180 },
                        scripts: [],
                      },
                      {
                        id: 'target-mc-a',
                        name: 'soundMcA',
                        transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'mc-sound-target', playback: 'independent' },
                        scripts: [],
                      },
                      {
                        id: 'target-mc-b',
                        name: 'soundMcB',
                        transform: { x: 10, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'mc-sound-target', playback: 'independent' },
                        scripts: [],
                      },
                    ],
                  },
                ],
              },
            ],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'action_script2',
                  source: `var as2RtmpCodes:Array = [];
var as2RtmpNc:NetConnection = new NetConnection();
as2RtmpNc.onStatus = function(info:Object):Void { as2RtmpCodes.push(info.code + ":" + info.level); };
var as2RtmpRet:Boolean = as2RtmpNc.connect("rtmp://live.example.test/as2");

var as2Nc:NetConnection = new NetConnection();
var as2Ns:NetStream = null;
var as2PubNs:NetStream = null;
var as2ConflictNs:NetStream = null;
var as2SubNs:NetStream = null;
var as2Ready:Boolean = false;
var as2Statuses:Array = [];
var as2PlayStatuses:Array = [];
var as2Metas:Array = [];
var as2Cues:Array = [];
var as2PubStatuses:Array = [];
var as2ConflictStatuses:Array = [];
var as2SubStatuses:Array = [];
var as2SubMetas:Array = [];
var as2SubMessages:Array = [];
var as2PubLiveDelay:Number = -1;
var as2SubLiveDelay:Number = -1;

as2Nc.onStatus = function(info:Object):Void {
  if (info.code == "NetConnection.Connect.Success") {
    as2Ns = new NetStream(as2Nc);
    as2Ns.onStatus = function(st:Object):Void { as2Statuses.push(st.code); };
    as2Ns.onPlayStatus = function(ps:Object):Void { as2PlayStatuses.push(ps.code); };
    as2Ns.onMetaData = function(md:Object):Void { as2Metas.push(md); };
    as2Ns.onCuePoint = function(cp:Object):Void { as2Cues.push(cp.name); };
    as2Video.attachVideo(as2Ns);

    as2PubNs = new NetStream(as2Nc);
    as2PubNs.onStatus = function(st:Object):Void { as2PubStatuses.push(st.code + ":" + st.level); };
    as2ConflictNs = new NetStream(as2Nc);
    as2ConflictNs.onStatus = function(st:Object):Void { as2ConflictStatuses.push(st.code + ":" + st.level); };
    as2SubNs = new NetStream(as2Nc);
    as2SubNs.onStatus = function(st:Object):Void { as2SubStatuses.push(st.code + ":" + st.level); };
    as2SubNs.onMetaData = function(md:Object):Void { as2SubMetas.push(md); };
    as2SubNs.onLiveTag = function(msg:String, seq:Number):Void { as2SubMessages.push(msg + ":" + seq); };
    as2Ready = true;
  }
};
this.startAs2PublishLoopback = function():Void {
  var mic:Microphone = Microphone.get();
  var cam:Camera = Camera.get();
  if (cam != null) cam.setMode(240, 180, 20);
  as2PubNs.attachAudio(mic);
  as2PubNs.attachVideo(cam, 80);
  as2PubNs.publish("studioFeed", "live");
  as2ConflictNs.publish("studioFeed", "live");
  as2Video.attachVideo(as2SubNs);
  as2SubNs.play("studioFeed");
  as2PubNs.send("onLiveTag", "beat", 7);
};
this.sampleAs2LiveDelaysAndUnpublish = function():Void {
  as2PubLiveDelay = as2PubNs.liveDelay;
  as2SubLiveDelay = as2SubNs.liveDelay;
  as2PubNs.attachAudio(false);
  as2PubNs.attachVideo(null);
  as2PubNs.publish(false);
  as2Video.attachVideo(as2Ns);
};
as2Nc.connect(null);

var s1:Sound = new Sound(soundMcA);
var s2:Sound = new Sound(soundMcA);
var sOther:Sound = new Sound(soundMcB);
s1.setVolume(60);
s1.setPan(-50);
var as2SharedCheck:Array = [s2.getVolume(), s2.getPan(), sOther.getVolume(), sOther.getPan()];
s2.setTransform({ ll: 25, lr: 75, rl: 50, rr: 10 });
var tfRead:Object = s1.getTransform();
var as2MatrixRead:Array = [tfRead.ll, tfRead.lr, tfRead.rl, tfRead.rr];
`,
                },
              },
            ],
          },
        },
      ],
    };

    const projFile = resolve(workDir, 'project.nflash');
    writeFileSync(projFile, JSON.stringify(project, null, 2));

    const exported = runCargo(
      cargo,
      ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', projFile, '--output', exportDir],
      { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 }
    );
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);

    const sandbox = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(readFileSync(resolve(exportDir, 'bundle.js'), 'utf8'), sandbox);
    const bundle = vm.runInContext('NeoFlashBundle', sandbox);
    vm.runInContext(readFileSync(resolve(exportDir, 'neoflash_web.js'), 'utf8'), sandbox);
    sandbox.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', sandbox);

    player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({ host } = createRuntimeHost(
      root,
      player,
      Array.from(bundle.scripts, (item) => ({
        script: { language: item.language, source: item.source },
        code: item.code,
        location: item.location,
      })),
      {
        bundle,
        net: { baseURL },
        audio: { createContext: harness.createAudioContext, createAudio: harness.createOpaqueAudio, isUserGesture: () => true },
        video: {
          createCanvas: harness.createCanvas,
          createVideo: harness.createVideo,
          createAudioContext: harness.createAudioContext,
          createObjectURL: harness.createObjectURL,
          revokeObjectURL: harness.revokeObjectURL,
          isUserGesture: () => true,
        },
        context: { measureText: (line) => ({ width: line.length * 8 }) },
      }
    ));

    host.dispatchScripts();
      let scene = host.proxyFor([]);
      await until(() => scene.ready && scene.as2Ready, 'AS3 and AS2 NetConnection(null) + NetStream initialize');
      await host.audio().resumeFromGesture();
      await host.video().resumeFromGesture();

      // -----------------------------------------------------------------------
      // A. Non-RTMP rejection diagnostics (AS3 & AS2)
      // -----------------------------------------------------------------------
      await until(
        () => Array.from(scene.rtmpStatusLog).includes('NetConnection.Connect.Rejected:error')
          && Array.from(scene.as2RtmpCodes).includes('NetConnection.Connect.Rejected:error'),
        'RTMP NetConnection.connect emits NetConnection.Connect.Rejected in AS3 & AS2'
      );
      assert.equal(scene.rtmpNc.connected, false);
      assert.equal(scene.as2RtmpRet, false);
      assert.equal(scene.nc.connected, true);

      scene.clearLogs();
      scene.ns.play('rtmp://live.example.test/app/liveStream');
      scene.as2Ns.play('rtmps://live.example.test/app/liveStream');
      await until(
        () => Array.from(scene.statusLog).includes('NetStream.Play.StreamNotFound')
          && Array.from(scene.as2Statuses).includes('NetStream.Play.StreamNotFound'),
        'RTMP NetStream.play emits NetStream.Play.StreamNotFound in AS3 & AS2'
      );

      scene.clearLogs();
      scene.as2Statuses.length = 0;
      scene.play2Stream('rtmp://live.example.test/app/play2Live');
      scene.as2Ns.play2({ streamName: 'rtmp://live.example.test/app/as2Play2Live' });
      await until(
        () => Array.from(scene.statusLog).includes('NetStream.Play.StreamNotFound')
          && Array.from(scene.as2Statuses).includes('NetStream.Play.StreamNotFound'),
        'RTMP NetStream.play2 emits NetStream.Play.StreamNotFound without Transition'
      );
      assert.ok(!Array.from(scene.statusLog).includes('NetStream.Play.Transition'), 'NetStream.play2 must not emit Transition on rtmp://');
      assert.equal(Array.from(scene.playStatusLog).length, 0, 'NetStream.play2 must not emit TransitionComplete on rtmp://');

      // -----------------------------------------------------------------------
      // B. AS2 Sound(mc) shared MovieClip target transform + 2x2 matrix state
      // -----------------------------------------------------------------------
      assert.deepEqual(Array.from(scene.as2SharedCheck), [60, -50, 100, 0], 'AS2 Sound instances sharing a MovieClip target share volume/pan while sibling MovieClip stays independent');
      assert.deepEqual(Array.from(scene.as2MatrixRead), [25, 75, 50, 10], 'AS2 Sound.setTransform/getTransform round-trips 2x2 channel matrix across shared MovieClip Sound instances');

      // -----------------------------------------------------------------------
      // C. NetStream.play(url, start, len=0, reset=true) single-frame load!
      // -----------------------------------------------------------------------
      scene.clearLogs();
      scene.playArgs('media/motion.mp4', 1.5, 0, true);
      await until(
        () => Array.from(scene.statusLog).includes('NetStream.Play.Stop')
          && Array.from(scene.playStatusLog).includes('NetStream.Play.Complete'),
        'len=0 single-frame load seeks to start=1.5s, renders frame, pauses, and completes'
      );
      const singleStatuses = Array.from(scene.statusLog);
      assert.deepEqual(
        singleStatuses,
        ['NetStream.Play.Start', 'NetStream.Buffer.Full', 'NetStream.Buffer.Empty', 'NetStream.Play.Stop'],
        'len=0 emits Start -> Buffer.Full -> Buffer.Empty -> Stop without spurious Seek.Notify'
      );
      assert.ok(Math.abs(scene.ns.time - 1.5) < 1e-3, 'single-frame play lands at start=1.5s');
      assert.equal(scene.ns.bytesLoaded, cuedMp4.length);
      assert.equal(scene.ns.bytesTotal, cuedMp4.length);

      // Verify Video paints the captured single frame!
      const paintStage = () => {
        const snap = host.video().prepareSnapshot(JSON.parse(player.runtime_snapshot_json()));
        const vItems = snap.draws.filter((d) => d.kind.type === 'video_frame');
        const ctx = harness.createCanvas(640, 360).getContext('2d');
        for (const item of vItems) host.video().draw(ctx, item.kind, item.owner_ref);
        return vItems;
      };
      assert.ok(paintStage().some((v) => v.kind.alpha_bounds !== null), 'Video renders decoded frame after len=0 single-frame play');

      // -----------------------------------------------------------------------
      // D. NetStream.play(url, start, len > 0, reset=true) + playlist queue (reset=false)!
      //    Queue clip 1 (video from 0.1s for len=0.5s, crossing chapter_intro cue @ 0.25s)
      //    followed by clip 2 (audio/cued.wav with reset=false, crossing beat_1 cue @ 0.15s)!
      // -----------------------------------------------------------------------
      scene.clearLogs();
      scene.playArgs('media/motion.mp4', 0.1, 0.5, true);
      scene.playArgs('audio/cued.wav', -2, -1, false);

      await until(() => harness.videoElements.at(-1)?.paused === false, 'clip 1 starts playing at start=0.1s');
      const clip1El = harness.videoElements.at(-1);
      assert.ok(Math.abs(clip1El.currentTime - 0.1) < 1e-3, 'clip 1 starts at 0.1s');
      assert.ok(scene.ns.currentFPS > 0, 'currentFPS > 0 while video clip is playing');
      assert.ok(scene.ns.bufferLength > 0, 'bufferLength > 0 from buffered range');
      scene.setReceiveAudio(false);
      assert.equal(clip1El.muted, true, 'receiveAudio(false) mutes decoded NetStream audio');
      scene.setReceiveAudio(true);
      assert.equal(clip1El.muted, false, 'receiveAudio(true) restores decoded NetStream audio');
      scene.setReceiveVideo(false);
      assert.equal(scene.ns.currentFPS, 0, 'receiveVideo(false) gates video FPS');
      assert.ok(paintStage().some((v) => v.kind.alpha_bounds !== null), 'receiveVideo(false) retains the last decoded video frame');
      scene.setReceiveVideo(true);
      assert.ok(scene.ns.currentFPS > 0, 'receiveVideo(true) resumes video FPS');
      assert.throws(() => scene.setReceiveVideoFPS(-1), />= 0/, 'receiveVideoFPS rejects negative rates');
      scene.setReceiveVideoFPS(0);
      assert.equal(scene.ns.currentFPS, 0, 'receiveVideoFPS(0) gates video delivery');
      assert.ok(paintStage().some((v) => v.kind.alpha_bounds !== null), 'receiveVideoFPS(0) retains the last decoded video frame');
      scene.setReceiveVideoFPS(12);
      assert.ok(scene.ns.currentFPS > 0, 'positive receiveVideoFPS resumes video delivery');
      assert.ok(paintStage().some((v) => v.kind.alpha_bounds !== null), 'positive receiveVideoFPS resumes the live video frame');

      // Advance 0.25s -> crosses 0.25s MP4 chapter cue ('chapter_intro'), currentTime = 0.35s (< 0.6s clip end)
      clip1El.advance(0.25, 1040);
      assert.ok(Array.from(scene.cueLog).includes('chapter_intro@0.25'), 'MP4 chapter cue point fires onCuePoint during playback');
      assert.equal(Array.from(scene.playStatusLog).length, 0, 'bounded clip has not ended before start + len (0.6s)');

      // Advance 0.30s -> reaches 0.60s (start 0.1 + len 0.5), triggering playlist transition to audio/cued.wav!
      clip1El.advance(0.30, 1080);
      await until(
        () => Array.from(scene.playStatusLog).includes('NetStream.Play.Switch')
          && Array.from(scene.metaLog).length === 2,
        'clip 1 finishes at start+len and transitions to queued audio clip 2 without dropping callbacks'
      );
      assert.ok(!Array.from(scene.statusLog).includes('NetStream.Play.Transition'), 'ordinary playlist switch is not a play2 bitrate transition');
      assert.ok(!Array.from(scene.playStatusLog).includes('NetStream.Play.TransitionComplete'), 'ordinary playlist switch does not report TransitionComplete');
      const secondMeta = scene.metaLog[1];
      assert.equal(secondMeta.width, 0, 'audio-only NetStream clip reports width=0');
      assert.equal(secondMeta.height, 0, 'audio-only NetStream clip reports height=0');
      assert.ok(secondMeta.duration > 0.4, 'audio-only NetStream clip reports accurate WAV duration');

      await until(() => harness.videoElements.at(-1)?.paused === false, 'queued audio clip 2 begins playing');
      const clip2El = harness.videoElements.at(-1);
      assert.equal(scene.ns.currentFPS, 0, 'audio-only NetStream reports currentFPS === 0');
      clip2El.advance(0.20, 1120);
      assert.ok(Math.abs(scene.ns.time - 0.8) < 1e-3, 'NetStream.time continues across playlist elements');
      assert.ok(Array.from(scene.cueLog).includes('beat_1@0.15'), 'WAV cue chunk fires onCuePoint during queued audio playback');
      clip2El.advance(0.40, 1160);
      await until(
        () => Array.from(scene.playStatusLog).includes('NetStream.Play.Complete')
          && Array.from(scene.statusLog).at(-1) === 'NetStream.Play.Stop',
        'queued clip 2 finishes naturally with Buffer.Empty -> Play.Stop + Play.Complete'
      );

      // -----------------------------------------------------------------------
      // E. Progressive HTTP 302 redirect + 206 Partial Content Range + Seek.Notify / Seek.InvalidTime + data: URI
      // -----------------------------------------------------------------------
      scene.clearLogs();
      scene.playArgs('/redirect-video.mp4', -1, -1, true);
      await until(
        () => Array.from(scene.metaLog).length === 1 && scene.ns.bytesTotal === cuedMp4.length,
        'HTTP 302 -> 206 Range probe resolves bytesTotal and metadata without throwing'
      );
      await until(() => harness.videoElements.at(-1)?.paused === false, 'progressive HTTP video starts');
      assert.ok(serverStats.redirects >= 1, '302 redirect was followed');
      assert.ok(serverStats.rangeRequests.some((r) => /^bytes=0-\d+$/.test(r)), '206 Range probe sent bounded bytes=0-N header');
      assert.ok(scene.ns.bytesLoaded > 0 && scene.ns.bytesLoaded <= scene.ns.bytesTotal, 'progressive bytesLoaded tracks buffered fraction');

      // Valid seek vs out-of-bounds Seek.InvalidTime
      scene.ns.seek(2.0);
      await until(() => Array.from(scene.statusLog).includes('NetStream.Seek.Notify'), 'valid seek(2.0) emits NetStream.Seek.Notify');
      scene.ns.seek(-3);
      await until(() => Array.from(scene.statusLog).includes('NetStream.Seek.InvalidTime'), 'negative seek(-3) emits NetStream.Seek.InvalidTime');
      assert.equal(scene.lastSeekDetails, 0);

      scene.clearLogs();
      scene.ns.seek(999);
      await until(() => Array.from(scene.statusLog).includes('NetStream.Seek.InvalidTime'), 'beyond-duration seek(999) emits NetStream.Seek.InvalidTime');
      assert.equal(scene.lastSeekDetails, 4);

      // Progressive len=0 (single-frame) and len>0 (bounded clip) preserve partial bytesLoaded < bytesTotal!
      scene.clearLogs();
      scene.playArgs('/range-video.mp4', 0, 0, true);
      await until(
        () => Array.from(scene.statusLog).includes('NetStream.Play.Stop') && scene.ns.bytesTotal === cuedMp4.length,
        'progressive len=0 single-frame completes and resolves bytesTotal'
      );
      assert.ok(
        scene.ns.bytesLoaded > 0 && scene.ns.bytesLoaded < scene.ns.bytesTotal,
        `progressive len=0 clip must preserve partial bytesLoaded (${scene.ns.bytesLoaded}) < bytesTotal (${scene.ns.bytesTotal})`
      );

      scene.clearLogs();
      scene.playArgs('/range-video.mp4', 0, 0.25, true);
      await until(() => harness.videoElements.at(-1)?.paused === false && scene.ns.bytesTotal === cuedMp4.length, 'progressive len=0.25 starts');
      harness.videoElements.at(-1).advance(0.3);
      await until(() => Array.from(scene.statusLog).includes('NetStream.Play.Stop'), 'progressive len=0.25 stops at clip end');
      assert.ok(
        scene.ns.bytesLoaded > 0 && scene.ns.bytesLoaded < scene.ns.bytesTotal,
        `progressive bounded len>0 clip must preserve partial bytesLoaded (${scene.ns.bytesLoaded}) < bytesTotal (${scene.ns.bytesTotal})`
      );

      // AS2 embedded data: URI + AS2 playlist / seek check
      scene.as2Statuses.length = 0;
      scene.as2PlayStatuses.length = 0;
      const dataUri = `data:video/mp4;base64,${cuedMp4.toString('base64')}`;
      scene.as2Ns.play(dataUri, 0.5, 0.5, true);
      await until(() => harness.videoElements.at(-1)?.paused === false, 'AS2 data: URI video starts at 0.5s');
      assert.equal(scene.as2Ns.bytesLoaded, cuedMp4.length);
      assert.equal(scene.as2Ns.bytesTotal, cuedMp4.length);
      harness.videoElements.at(-1).advance(0.6, 1300);
      await until(
        () => Array.from(scene.as2Statuses).includes('NetStream.Play.Stop')
          && Array.from(scene.as2PlayStatuses).includes('NetStream.Play.Complete'),
        'AS2 bounded data: URI playback stops at start+len and fires onPlayStatus(Complete)'
      );

      // AS2 NetStream.{attachAudio, attachVideo, publish, liveDelay} live loopback verification
      scene.startAs2PublishLoopback();
      await until(
        () => Array.from(scene.as2PubStatuses).includes('NetStream.Publish.Start:status')
          && Array.from(scene.as2ConflictStatuses).includes('NetStream.Publish.BadName:error')
          && Array.from(scene.as2SubStatuses).includes('NetStream.Play.Start:status')
          && Array.from(scene.as2SubMetas).length >= 1
          && Array.from(scene.as2SubMessages).includes('beat:7'),
        'AS2 NetStream publish/play loopback delivers Publish.Start, BadName, subscriber metadata & send() messages'
      );
      scene.sampleAs2LiveDelaysAndUnpublish();
      assert.equal(typeof scene.as2PubLiveDelay, 'number');
      assert.equal(typeof scene.as2SubLiveDelay, 'number');
      assert.ok(scene.as2PubLiveDelay > 0, `expected positive publisher liveDelay, got ${scene.as2PubLiveDelay}`);
      assert.ok(scene.as2SubLiveDelay > 0, `expected positive subscriber liveDelay, got ${scene.as2SubLiveDelay}`);
      assert.equal(scene.as2SubMetas[0].live, true);
      await until(
        () => Array.from(scene.as2PubStatuses).includes('NetStream.Unpublish.Success:status')
          && Array.from(scene.as2SubStatuses).includes('NetStream.Play.UnpublishNotify:status'),
        'AS2 NetStream publish(false) emits Unpublish.Success and subscriber UnpublishNotify'
      );

      // -----------------------------------------------------------------------
      // F. SoundTransform Web Audio StereoPannerNode & 2x2 Matrix routing:
      //    SoundChannel, SoundMixer, NetStream, and ancestor Sprite.soundTransform!
      // -----------------------------------------------------------------------
      const Sound = host.audio().Sound;
      const SoundTransform = host.audio().SoundTransform;
      const SoundLoaderContext = host.audio().SoundLoaderContext;
      const SoundMixer = host.audio().SoundMixer;
      const URLRequest = host.net().URLRequest;
      const ByteArray = host.program().type('flash.utils.ByteArray');

      // 1) NetStream pure pan -> StereoPannerNode, then custom 2x2 matrix -> Splitter/4xGain/Merger, composed with parentHolder.soundTransform!
      scene.playArgs('media/motion.mp4', 0, -1, true);
      await until(() => harness.videoElements.at(-1)?.paused === false, 'stream active for Web Audio routing check');
      scene.ns.soundTransform = new SoundTransform(0.8, -0.5);
      const audioCtx = harness.audioContexts[0];
      assert.ok(audioCtx.mediaElementSources.length >= 1, 'NetStream nonzero pan attaches MediaElementAudioSourceNode');
      assert.ok(Math.abs(audioCtx.panners.at(-1).pan.value + 0.5) < 1e-5, 'StereoPannerNode receives pan = -0.5');

      scene.setParentVolumePan(0.5, 0);
      scene.setStreamMatrix(0.2, 0.8, 0.6, 0.4);
      paintStage(); // syncs ancestor Sprite.soundTransform into attached NetStream!
      assert.ok(audioCtx.splitters.length >= 1 && audioCtx.mergers.length >= 1, 'custom 2x2 matrix engages ChannelSplitterNode + ChannelMergerNode');
      // Stream [ll=0.2, rl=0.6, lr=0.8, rr=0.4] * Parent(vol=0.5) -> [0.1, 0.3, 0.4, 0.2]
      const streamMatrixGains = audioCtx.gains.slice(-4).map((g) => Math.round(g.gain.value * 1000) / 1000);
      assert.deepEqual(streamMatrixGains, [0.1, 0.3, 0.4, 0.2], 'NetStream 2x2 matrix composes with parent Sprite.soundTransform');
      scene.setParentVolumePan(1, 0);

      // 2) SoundChannel 2x2 matrix + live SoundMixer.soundTransform composition
      const corsSound = new Sound();
      let corsDone = false;
      corsSound.addEventListener('complete', () => { corsDone = true; });
      corsSound.load(new URLRequest(`${mainOrigin}/redirect-cors-audio.wav`), new SoundLoaderContext(1000, true));
      await until(() => corsDone, 'Sound.load follows 302 redirect to CORS-enabled WAV');
      assert.equal(corsSound.isURLInaccessible, false, 'CORS-enabled redirected audio has isURLInaccessible === false');
      assert.equal(corsSound.url, `${corsOrigin}/cors-tone.wav`, 'Sound.url updates to final redirected URL');

      const chMatrixTf = new SoundTransform(1, 0);
      chMatrixTf.leftToLeft = 0;
      chMatrixTf.leftToRight = 1;
      chMatrixTf.rightToLeft = 0.5;
      chMatrixTf.rightToRight = 0;
      const ch = corsSound.play(0, 0, chMatrixTf);
      const chGains = audioCtx.gains.slice(-4);
      assert.deepEqual(
        chGains.map((g) => Math.round(g.gain.value * 1000) / 1000),
        [0, 0.5, 1, 0],
        'SoundChannel custom L<->R swap installs exact 2x2 Web Audio matrix gains [ll, rl, lr, rr]'
      );
      SoundMixer.soundTransform = new SoundTransform(0.5, 0);
      assert.deepEqual(
        chGains.map((g) => Math.round(g.gain.value * 1000) / 1000),
        [0, 0.25, 0.5, 0],
        'mutating SoundMixer.soundTransform live-scales active 2x2 SoundChannel matrix gains'
      );
      SoundMixer.soundTransform = new SoundTransform(1, 0);
      ch.stop();

      // -----------------------------------------------------------------------
      // G. Sound.isURLInaccessible & SecurityError enforcement on cross-origin non-CORS audio
      // -----------------------------------------------------------------------
      const restrictedSound = new Sound();
      let restrictedDone = false;
      restrictedSound.addEventListener('complete', () => { restrictedDone = true; });
      restrictedSound.load(new URLRequest(`${restrictedOrigin}/secret.wav`));
      await until(() => restrictedDone, 'cross-origin non-CORS Sound falls back to opaque HTMLAudioElement and completes');
      assert.equal(restrictedSound.isURLInaccessible, true, 'cross-origin Sound without ACAO sets isURLInaccessible = true');

      const secTarget = new ByteArray();
      assert.throws(
        () => restrictedSound.extract(secTarget, 64, 0),
        /SecurityError|#2122/i,
        'Sound.extract throws SecurityError #2122 when isURLInaccessible is true'
      );

      const restrictedCh = restrictedSound.play(0, 0, new SoundTransform(0.5, 0));
      assert.equal(SoundMixer.areSoundsInaccessible(), true, 'SoundMixer.areSoundsInaccessible() is true while opaque channel plays');
      assert.throws(
        () => SoundMixer.computeSpectrum(secTarget, false, 0),
        /SecurityError|#2121/i,
        'SoundMixer.computeSpectrum throws SecurityError #2121 while inaccessible audio plays'
      );
      restrictedCh.stop();
      assert.equal(SoundMixer.areSoundsInaccessible(), false, 'SoundMixer.areSoundsInaccessible() returns false after stopping opaque channel');

      // -----------------------------------------------------------------------
      // H. Pause, Close, Restart & Resource Cleanup (probes, blobs, elements, Web Audio nodes)
      // -----------------------------------------------------------------------
      scene.ns.play('/slow-video.mp4');
      await until(() => serverStats.slowHits >= 1, '/slow-video.mp4 request reached HTTP server');
      scene.ns.close();
      scene.as2Ns.close();
      await until(() => serverStats.slowAborted >= 1, 'NetStream.close() aborts in-flight /slow-video.mp4 fetch');
      assert.ok(serverStats.slowAborted >= 1, 'server observed aborted /slow-video.mp4 connection');
      assert.equal(scene.ns.time, 0);
      assert.equal(harness.blobs.size, 0, 'all Blob object URLs revoked after close()');
      assert.ok(harness.revoked.length >= 2, 'URL.revokeObjectURL called for embedded/data blobs');

      const opaqueChBeforeReset = restrictedSound.play(0, 0, new SoundTransform(0.4, 0));
      assert.ok(opaqueChBeforeReset);
      host.resetProject();
      scene = host.proxyFor([]);
      await until(() => scene.ready && scene.as2Ready, 'Restart re-initializes fresh AS3 & AS2 streams');
      assert.equal(harness.blobs.size, 0, 'zero unrevoked Blob URLs after Restart');
      assert.ok(
        harness.videoElements.every((el) => el.paused && el.src === ''),
        'every prior <video> element is paused and detached (src === "") after Restart'
      );
      assert.ok(
        harness.audioElements.every((el) => el.paused && el.src === ''),
        'every opaque <audio> element is paused and detached (src === "") after Restart'
      );

      host.disposeFamily();
      host = null;
      player.free();
      player = null;
  } finally {
    host?.disposeFamily();
    player?.free();
    for (const srv of [mainServer, corsServer, restrictedServer]) {
      if (srv) {
        srv.closeAllConnections?.();
        await new Promise((done) => srv.close(done));
      }
    }
    rmSync(workDir, { recursive: true, force: true });
  }
  console.log('Media & NetStream (T-327): AS3/AS2 non-RTMP NetStream.play(url, start, len, reset), playlist transitions, 206 Range + 302 redirects + data: URIs, container cues, SoundTransform 2x2 matrix & StereoPanner, Sound.isURLInaccessible CORS enforcement, and Restart cleanup passed.');
}

const isDirectRun = process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isDirectRun) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (existsSync(localCargo) ? localCargo : 'cargo');
  await verifyMediaNetStream(root, cargo);
}
