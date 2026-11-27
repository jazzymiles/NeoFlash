#!/usr/bin/env node
/**
 * Permanent Firefox HTML5 compatibility gate (T-324).
 *
 * Drives a real local Firefox binary in headless mode over native WebDriver BiDi
 * (zero external npm dependencies) using an isolated disposable Firefox profile
 * that preserves Firefox's realistic default `file://` strict origin policy.
 *
 * Matrix verified:
 *   1. SizeLevel::Full     over file://  (Canvas + DOM)
 *   2. SizeLevel::Balanced over file://  (Canvas + DOM)
 *   3. SizeLevel::Compact  over file://  (Canvas + DOM, scripted + scriptless trimmed)
 *   4. SizeLevel::Shared   over http://127.0.0.1:<port> (Canvas + DOM, external shared runtime)
 *
 * Capabilities verified with real WASM state, real BiDi input actions, and real Stage pixels:
 *   - AS2 frame/instance scripts + nested MovieClip timelines advancing with verified pixel changes
 *   - AS3 document/linked classes + bubbling event dispatch + nested MovieClip timelines
 *   - Text focus & input: pointer click to focus, keyboard selection + replacement, WASM & Stage pixels
 *   - Embedded fonts: @font-face / document.fonts.check + non-blank glyph pixels on Canvas & DOM
 *   - Audio unlock: real user-gesture pointer/key activation unlocking Firefox AudioContext
 *   - Video pixels: embedded & HTTP MP4 decoding non-zero frame pixels onto Canvas & DOM without taint
 *   - URL loading: AS3 URLLoader & AS2 LoadVars over HTTP + graceful non-crashing handling on file://
 *   - Pointer activation: hover + click on interactive Button & MovieClip with exact stage coordinates
 *   - Restart & unload: player Restart resets timeline/scripts/text/media; pagehide/unload tears down
 *     RAF, ResizeObserver, audio, video, DOM stage, and timers with 0 console/page errors
 */

import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import zlib from 'node:zlib';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
export const ROOT_DIR = path.resolve(SCRIPT_DIR, '..');
const DEFAULT_OUT_DIR = path.join(ROOT_DIR, 'target', 'firefox-gate');
const RUNTIME_DIR = path.join(ROOT_DIR, 'target', 'web-runtime');
const EXPORTER_BIN = path.join(
  ROOT_DIR,
  'target',
  'debug',
  process.platform === 'win32' ? 'neoflash-export.exe' : 'neoflash-export',
);

const EMBEDDED_FONT_FAMILY = 'NeoFlashGateEmbedded';

function augmentedEnv(extra = {}) {
  const sep = process.platform === 'win32' ? ';' : ':';
  const prefixes = [
    path.join(os.homedir(), '.cargo', 'bin'),
    '/opt/homebrew/bin',
    '/usr/local/bin',
  ].filter(p => fs.existsSync(p));
  const currentPath = process.env.PATH || '';
  return {
    ...process.env,
    ...extra,
    PATH: [...prefixes, currentPath].filter(Boolean).join(sep),
  };
}

export function resolveFirefoxBinary() {
  const explicit = process.env.NEOFLASH_GATE_FIREFOX || process.env.FIREFOX_BIN || '';
  if (explicit.trim()) {
    const candidate = explicit.trim();
    if (fs.existsSync(candidate)) return candidate;
    return null;
  }

  const candidates = [];
  if (process.platform === 'darwin') {
    candidates.push(
      '/Applications/Firefox.app/Contents/MacOS/firefox',
      '/Applications/Firefox Developer Edition.app/Contents/MacOS/firefox',
      '/Applications/Firefox Nightly.app/Contents/MacOS/firefox',
      path.join(os.homedir(), 'Applications/Firefox.app/Contents/MacOS/firefox'),
    );
  } else if (process.platform === 'win32') {
    candidates.push(
      'C:\\Program Files\\Mozilla Firefox\\firefox.exe',
      'C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe',
    );
  } else {
    candidates.push('/usr/bin/firefox', '/usr/local/bin/firefox', '/snap/bin/firefox');
  }

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
  }

  const pathDirs = (process.env.PATH || '').split(process.platform === 'win32' ? ';' : ':');
  const exeName = process.platform === 'win32' ? 'firefox.exe' : 'firefox';
  for (const dir of pathDirs) {
    if (!dir) continue;
    const full = path.join(dir, exeName);
    if (fs.existsSync(full)) return full;
  }
  return null;
}

function newestMtimeMs(targetPath) {
  if (!fs.existsSync(targetPath)) return 0;
  const stat = fs.statSync(targetPath);
  if (!stat.isDirectory()) return stat.mtimeMs;
  let newest = stat.mtimeMs;
  for (const entry of fs.readdirSync(targetPath, { withFileTypes: true })) {
    if (entry.name === 'tests' || entry.name === 'node_modules') continue;
    const sub = newestMtimeMs(path.join(targetPath, entry.name));
    if (sub > newest) newest = sub;
  }
  return newest;
}

function ensureBuildArtifacts(quiet = false) {
  const env = augmentedEnv();
  const stdio = quiet ? 'pipe' : 'inherit';

  const wasmFile = path.join(RUNTIME_DIR, 'neoflash_web_bg.wasm');
  const runtimeJs = path.join(RUNTIME_DIR, 'neoflash_web.js');
  const webSrcNewest = Math.max(
    newestMtimeMs(path.join(ROOT_DIR, 'web')),
    newestMtimeMs(path.join(ROOT_DIR, 'crates', 'neoflash-web', 'src')),
  );
  const runtimeBuiltMtime = Math.min(newestMtimeMs(wasmFile), newestMtimeMs(runtimeJs));

  if (!fs.existsSync(wasmFile) || !fs.existsSync(runtimeJs) || runtimeBuiltMtime < webSrcNewest) {
    if (!quiet) console.log('[firefox-gate] Building web runtime (./scripts/build-web.sh)...');
    execFileSync(path.join(ROOT_DIR, 'scripts', 'build-web.sh'), [], {
      cwd: ROOT_DIR,
      env,
      stdio,
    });
  }

  const exportSrcNewest = Math.max(
    webSrcNewest,
    newestMtimeMs(path.join(ROOT_DIR, 'crates', 'neoflash-export', 'src')),
    newestMtimeMs(path.join(ROOT_DIR, 'crates', 'neoflash-script', 'src')),
    newestMtimeMs(path.join(ROOT_DIR, 'crates', 'neoflash-core', 'src')),
  );
  if (!fs.existsSync(EXPORTER_BIN) || newestMtimeMs(EXPORTER_BIN) < exportSrcNewest) {
    if (!quiet) console.log('[firefox-gate] Building neoflash-export...');
    execFileSync(
      'cargo',
      ['build', '--locked', '-p', 'neoflash-export', '--bin', 'neoflash-export'],
      { cwd: ROOT_DIR, env, stdio },
    );
  }
}

// ---------------------------------------------------------------------------
// Pure-JS PNG decoder (built-in node:zlib) for real Gecko screenshot pixels
// ---------------------------------------------------------------------------
function paethPredictor(a, b, c) {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  if (pa <= pb && pa <= pc) return a;
  if (pb <= pc) return b;
  return c;
}

export function decodePngRgba(pngBuffer) {
  const sig = [137, 80, 78, 71, 13, 10, 26, 10];
  for (let i = 0; i < 8; i++) {
    if (pngBuffer[i] !== sig[i]) throw new Error('Invalid PNG signature in screenshot');
  }
  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idatChunks = [];

  while (offset + 8 <= pngBuffer.length) {
    const length = pngBuffer.readUInt32BE(offset);
    const type = pngBuffer.toString('ascii', offset + 4, offset + 8);
    const dataStart = offset + 8;
    const dataEnd = dataStart + length;
    if (type === 'IHDR') {
      width = pngBuffer.readUInt32BE(dataStart);
      height = pngBuffer.readUInt32BE(dataStart + 4);
      bitDepth = pngBuffer[dataStart + 8];
      colorType = pngBuffer[dataStart + 9];
    } else if (type === 'IDAT') {
      idatChunks.push(pngBuffer.subarray(dataStart, dataEnd));
    } else if (type === 'IEND') {
      break;
    }
    offset = dataEnd + 4;
  }

  if (bitDepth !== 8 || (colorType !== 6 && colorType !== 2)) {
    throw new Error(`Unsupported screenshot PNG format: depth=${bitDepth}, colorType=${colorType}`);
  }
  const bpp = colorType === 6 ? 4 : 3;
  const stride = width * bpp;
  const raw = zlib.inflateSync(Buffer.concat(idatChunks));
  const recon = new Uint8Array(height * stride);

  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const rowIn = y * (stride + 1) + 1;
    const rowOut = y * stride;
    const prevOut = (y - 1) * stride;
    for (let x = 0; x < stride; x++) {
      const byte = raw[rowIn + x];
      const left = x >= bpp ? recon[rowOut + x - bpp] : 0;
      const up = y > 0 ? recon[prevOut + x] : 0;
      const upLeft = y > 0 && x >= bpp ? recon[prevOut + x - bpp] : 0;
      let val = byte;
      if (filter === 1) val = (byte + left) & 0xff;
      else if (filter === 2) val = (byte + up) & 0xff;
      else if (filter === 3) val = (byte + ((left + up) >> 1)) & 0xff;
      else if (filter === 4) val = (byte + paethPredictor(left, up, upLeft)) & 0xff;
      recon[rowOut + x] = val;
    }
  }

  if (bpp === 4) {
    return { width, height, data: recon };
  }
  const rgba = new Uint8Array(width * height * 4);
  for (let i = 0, j = 0; i < recon.length; i += 3, j += 4) {
    rgba[j] = recon[i];
    rgba[j + 1] = recon[i + 1];
    rgba[j + 2] = recon[i + 2];
    rgba[j + 3] = 255;
  }
  return { width, height, data: rgba };
}

function samplePngRegion(png, rect, predicate) {
  const x0 = Math.max(0, Math.floor(rect.x));
  const y0 = Math.max(0, Math.floor(rect.y));
  const x1 = Math.min(png.width, Math.ceil(rect.x + rect.width));
  const y1 = Math.min(png.height, Math.ceil(rect.y + rect.height));
  let matched = 0;
  let total = 0;
  let sumR = 0;
  let sumG = 0;
  let sumB = 0;
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      const idx = (y * png.width + x) * 4;
      const r = png.data[idx];
      const g = png.data[idx + 1];
      const b = png.data[idx + 2];
      const a = png.data[idx + 3];
      total++;
      sumR += r;
      sumG += g;
      sumB += b;
      if (predicate(r, g, b, a)) matched++;
    }
  }
  return {
    total,
    matched,
    avg: total > 0 ? [Math.round(sumR / total), Math.round(sumG / total), Math.round(sumB / total)] : [0, 0, 0],
  };
}

// ---------------------------------------------------------------------------
// Fixture generation (uses real tracked TTF, WAV, and H.264 MP4 bytes)
// ---------------------------------------------------------------------------
function loadFixtureAssets() {
  const fontBytes = fs.readFileSync(
    path.join(ROOT_DIR, 'crates', 'neoflash-ui', 'assets', 'fonts', 'Inter-Regular.ttf'),
  );
  const soundExample = JSON.parse(
    fs.readFileSync(
      path.join(ROOT_DIR, 'examples', 'video-and-audio', 'timeline-sound.nflash'),
      'utf8',
    ),
  );
  const beepWavBase64 = soundExample.resources.find(r => r.id === 'beep').data_base64;

  const videoExample = JSON.parse(
    fs.readFileSync(
      path.join(ROOT_DIR, 'examples', 'video-and-audio', 'video-playback.nflash'),
      'utf8',
    ),
  );
  const motionMp4Base64 = videoExample.resources.find(r => r.id === 'original-motion').data_base64;

  const fontMetadata = [
    {
      id: 'gate-font-ttf',
      name: `${EMBEDDED_FONT_FAMILY} Regular`,
      family: EMBEDDED_FONT_FAMILY,
      style: 'Regular',
      ranges: [true, true, true, true, true, false, false, false],
      extra_chars: '',
      export_for_actionscript: false,
      class_name: '',
    },
  ];

  return {
    fontBase64: fontBytes.toString('base64'),
    fontMetadataBase64: Buffer.from(JSON.stringify(fontMetadata), 'utf8').toString('base64'),
    beepWavBase64,
    motionMp4Base64,
    motionMp4Bytes: Buffer.from(motionMp4Base64, 'base64'),
  };
}

function rectElement(id, x, y, width, height, fill, scripts = []) {
  return {
    id,
    name: id,
    transform: { x, y, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
    kind: {
      type: 'rectangle',
      width,
      height,
      fill,
      stroke: [0, 0, 0, 0],
      stroke_width: 0,
    },
    scripts,
  };
}

function buildSuiteProject(assets) {
  const as3SpinnerSource = `package gate {
  import flash.display.MovieClip;
  import flash.events.Event;
  public dynamic class As3Spinner extends MovieClip {
    public var phase:String = "init";
    public var transitions:int = 0;
    public function As3Spinner() {
      super();
    }
    public function reachPhase(nextPhase:String):void {
      phase = nextPhase;
      transitions++;
      dispatchEvent(new Event("spinnerPhase", true));
    }
  }
}`;

  const as3MainSource = `package gate {
  import flash.display.MovieClip;
  import flash.events.Event;
  import flash.events.FocusEvent;
  import flash.events.IOErrorEvent;
  import flash.events.MouseEvent;
  import flash.events.NetStatusEvent;
  import flash.events.SecurityErrorEvent;
  import flash.events.TextEvent;
  import flash.media.SoundTransform;
  import flash.media.Video;
  import flash.net.NetConnection;
  import flash.net.NetStream;
  import flash.net.URLLoader;
  import flash.net.URLRequest;
  import flash.text.TextField;
  import flash.text.TextFieldType;
  import flash.text.TextFormat;

  public dynamic class Main extends MovieClip {
    public var inputField:TextField;
    public var video:Video;
    public var connection:NetConnection;
    public var stream:NetStream;
    public var maxRootFrame:int = 1;
    public var spinnerEvents:int = 0;
    public var spinnerPhase:String = "init";
    public var focusIns:int = 0;
    public var textInputs:int = 0;
    public var textChanges:int = 0;
    public var btnRollOvers:int = 0;
    public var btnClicks:int = 0;
    public var btnHoverCoords:String = "";
    public var btnClickCoords:String = "";
    public var mcRollOvers:int = 0;
    public var mcClicks:int = 0;
    public var mcHoverCoords:String = "";
    public var mcClickCoords:String = "";
    public var videoReady:Boolean = false;
    public var videoMetaCount:int = 0;
    public var videoDuration:Number = 0;
    public var videoLastCode:String = "idle";
    public var videoSourcePlayed:String = "";
    public var bundledUrlStatus:String = "pending";
    public var siblingUrlStatus:String = "idle";

    public function Main() {
      super();
      addEventListener("spinnerPhase", onSpinnerPhase);
      addEventListener(Event.ENTER_FRAME, onTick);

      inputField = new TextField();
      inputField.name = "inputField";
      inputField.x = 40;
      inputField.y = 148;
      inputField.width = 360;
      inputField.height = 42;
      inputField.defaultTextFormat = new TextFormat("${EMBEDDED_FONT_FAMILY}", 22, 0x0f172a);
      inputField.embedFonts = true;
      inputField.type = TextFieldType.INPUT;
      inputField.background = true;
      inputField.backgroundColor = 0xf4f8fb;
      inputField.border = true;
      inputField.borderColor = 0x3b82f6;
      inputField.text = "ABCD";
      inputField.addEventListener(FocusEvent.FOCUS_IN, onInputFocusIn);
      inputField.addEventListener(TextEvent.TEXT_INPUT, onInputTyped);
      inputField.addEventListener(Event.CHANGE, onInputChanged);
      addChild(inputField);

      this.actionBtn.addEventListener(MouseEvent.ROLL_OVER, onBtnOver);
      this.actionBtn.addEventListener(MouseEvent.CLICK, onBtnClick);

      this.netMc.buttonMode = true;
      this.netMc.addEventListener(MouseEvent.ROLL_OVER, onMcOver);
      this.netMc.addEventListener(MouseEvent.CLICK, onMcClick);

      video = new Video(320, 180);
      video.name = "gateVideo";
      video.x = 440;
      video.y = 320;
      video.smoothing = true;
      addChild(video);

      connection = new NetConnection();
      connection.addEventListener(NetStatusEvent.NET_STATUS, onNetConnection);
      connection.connect(null);

      loadBundledNote();
      refreshStatus();
    }

    public function onNetConnection(event:NetStatusEvent):void {
      if (event.info.code == "NetConnection.Connect.Success") {
        stream = new NetStream(connection);
        stream.client = this;
        stream.soundTransform = new SoundTransform(0);
        stream.addEventListener(NetStatusEvent.NET_STATUS, onStreamStatus);
        video.attachNetStream(stream);
        videoReady = true;
      }
    }

    public function onMetaData(info:Object):void {
      videoMetaCount++;
      videoDuration = Number(info.duration);
      refreshStatus();
    }

    public function onStreamStatus(event:NetStatusEvent):void {
      videoLastCode = String(event.info.code);
      refreshStatus();
    }

    public function onSpinnerPhase(event:Event):void {
      spinnerEvents++;
      spinnerPhase = String(this.as3Nested.phase);
      refreshStatus();
    }

    public function onTick(event:Event):void {
      if (currentFrame > maxRootFrame) {
        maxRootFrame = currentFrame;
      }
    }

    public function onInputFocusIn(event:FocusEvent):void {
      focusIns++;
      refreshStatus();
    }

    public function onInputTyped(event:TextEvent):void {
      textInputs++;
      refreshStatus();
    }

    public function onInputChanged(event:Event):void {
      textChanges++;
      refreshStatus();
    }

    public function onBtnOver(event:MouseEvent):void {
      btnRollOvers++;
      btnHoverCoords = Math.round(event.stageX) + "," + Math.round(event.stageY);
      refreshStatus();
    }

    public function onBtnClick(event:MouseEvent):void {
      btnClicks++;
      btnClickCoords = Math.round(event.stageX) + "," + Math.round(event.stageY);
      this.as3Nested.gotoAndPlay(4);
      playVideoUrl("media/motion.mp4");
      refreshStatus();
    }

    public function onMcOver(event:MouseEvent):void {
      mcRollOvers++;
      mcHoverCoords = Math.round(event.stageX) + "," + Math.round(event.stageY);
      refreshStatus();
    }

    public function onMcClick(event:MouseEvent):void {
      mcClicks++;
      mcClickCoords = Math.round(event.stageX) + "," + Math.round(event.stageY);
      loadSiblingPayload();
      refreshStatus();
    }

    public function playVideoUrl(url:String):void {
      if (!videoReady) return;
      videoSourcePlayed = url;
      stream.play(url);
    }

    public function loadBundledNote():void {
      var loader:URLLoader = new URLLoader();
      loader.addEventListener(Event.COMPLETE, function(e:Event):void {
        bundledUrlStatus = "ok:" + String(loader.data);
        refreshStatus();
      });
      loader.addEventListener(IOErrorEvent.IO_ERROR, function(e:IOErrorEvent):void {
        bundledUrlStatus = "err";
        refreshStatus();
      });
      loader.load(new URLRequest("data/bundled-note.txt"));
    }

    public function loadSiblingPayload():void {
      siblingUrlStatus = "loading";
      var loader:URLLoader = new URLLoader();
      loader.addEventListener(Event.COMPLETE, function(e:Event):void {
        siblingUrlStatus = "ok:" + String(loader.data);
        refreshStatus();
      });
      loader.addEventListener(IOErrorEvent.IO_ERROR, function(e:IOErrorEvent):void {
        siblingUrlStatus = "io_error_handled";
        refreshStatus();
      });
      loader.addEventListener(SecurityErrorEvent.SECURITY_ERROR, function(e:SecurityErrorEvent):void {
        siblingUrlStatus = "security_error_handled";
        refreshStatus();
      });
      loader.load(new URLRequest("sibling-payload.json"));
    }

    public function refreshStatus():void {
      if (this.statusBox) {
        this.statusBox.text = "F=" + currentFrame + "/" + maxRootFrame
          + " | AS3=" + spinnerPhase + "(" + spinnerEvents + ")"
          + " | In=" + inputField.text
          + " | Btn=" + btnClicks + "@" + btnClickCoords
          + " | Mc=" + mcClicks + "@" + mcClickCoords
          + " | Vid=" + videoLastCode + "(" + videoMetaCount + ")"
          + " | Url=" + siblingUrlStatus;
      }
    }

    public function getGateSnapshot():Object {
      return {
        currentFrame: currentFrame,
        maxRootFrame: maxRootFrame,
        as2RootFrame1: this.as2RootFrame1,
        as2RootStoppedFrame: this.as2RootStoppedFrame,
        as2InstanceLoads: this.as2InstanceLoads,
        as2NestedPhase: this.as2NestedPhase,
        as2NestedVisits: this.as2NestedVisits,
        as2BtnClicks: this.as2BtnClicks,
        as2BtnMouse: this.as2BtnMouse,
        as2McClicks: this.as2McClicks,
        as2HoverCount: this.as2HoverCount,
        as2HoverMouse: this.as2HoverMouse,
        as2BundledVars: this.as2BundledVars,
        as2SiblingVars: this.as2SiblingVars,
        spinnerEvents: spinnerEvents,
        spinnerPhase: spinnerPhase,
        as3NestedFrame: this.as3Nested.currentFrame,
        focusIns: focusIns,
        textInputs: textInputs,
        textChanges: textChanges,
        inputText: inputField.text,
        selBegin: inputField.selectionBeginIndex,
        selEnd: inputField.selectionEndIndex,
        caretIndex: inputField.caretIndex,
        btnRollOvers: btnRollOvers,
        btnClicks: btnClicks,
        btnHoverCoords: btnHoverCoords,
        btnClickCoords: btnClickCoords,
        mcRollOvers: mcRollOvers,
        mcClicks: mcClicks,
        mcHoverCoords: mcHoverCoords,
        mcClickCoords: mcClickCoords,
        videoReady: videoReady,
        videoMetaCount: videoMetaCount,
        videoDuration: videoDuration,
        videoLastCode: videoLastCode,
        videoSourcePlayed: videoSourcePlayed,
        bundledUrlStatus: bundledUrlStatus,
        siblingUrlStatus: siblingUrlStatus
      };
    }
  }
}`;

  const rootAs2Frame0 = [
    'this.as2RootFrame1 = (this.as2RootFrame1 ? this.as2RootFrame1 : 0) + 1;',
    'this.as2HoverCount = 0;',
    'this.as2HoverMouse = "";',
    'this.as2BundledVars = "pending";',
    'this.as2SiblingVars = "idle";',
    'this.netMc.onRollOver = function() {',
    '  _root.as2HoverCount = _root.as2HoverCount + 1;',
    '  _root.as2HoverMouse = Math.round(_root._xmouse) + "," + Math.round(_root._ymouse);',
    '};',
    'var bundledLv:LoadVars = new LoadVars();',
    'bundledLv.onLoad = function(ok:Boolean) {',
    '  _root.as2BundledVars = ok ? (this.bundled + ":" + this.mode) : "failed";',
    '};',
    'bundledLv.load("data/bundled-vars.txt");',
    'this.runAs2LoadVars = function() {',
    '  var sibLv:LoadVars = new LoadVars();',
    '  sibLv.onLoad = function(ok:Boolean) {',
    '    _root.as2SiblingVars = ok ? ("ok:" + this.status + ":" + this.engine) : "io_error_handled";',
    '  };',
    '  sibLv.load("sibling-vars.txt");',
    '};',
  ].join('\n');

  return {
    format_version: 12,
    name: 'Firefox HTML5 Gate Suite',
    width: 800,
    height: 600,
    fps: 24,
    background: [14, 22, 34, 255],
    root: 'scene',
    document_class: 'gate.Main',
    symbols: [
      {
        id: 'scene',
        name: 'Main Scene',
        kind: 'movie_clip',
        timeline: {
          frame_count: 4,
          layers: [
            {
              id: 'sound-layer',
              name: 'Sound',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  elements: [],
                  tween: null,
                  label: '',
                  sound: {
                    resource: 'gate-beep',
                    sync: 'start',
                  },
                },
              ],
            },
            {
              id: 'content-layer',
              name: 'Content',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: 'boot',
                  elements: [
                    rectElement('font-card-bg', 360, 24, 400, 76, [20, 32, 48, 255]),
                    {
                      id: 'embedded-headline',
                      name: 'embeddedHeadline',
                      transform: { x: 374, y: 38, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'text_box',
                        text: 'GATE FONT 88',
                        width: 370,
                        height: 50,
                        format: {
                          font: EMBEDDED_FONT_FAMILY,
                          size: 32,
                          color: 0x55ff99,
                          bold: false,
                          italic: false,
                          leading: 2,
                          align: 'left',
                          fieldOptions: {
                            embed_fonts: true,
                            selectable: false,
                          },
                        },
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    rectElement('video-backdrop', 436, 316, 328, 188, [23, 38, 56, 255]),
                    {
                      id: 'status-box',
                      name: 'statusBox',
                      transform: { x: 40, y: 330, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'text_box',
                        text: 'Booting...',
                        width: 370,
                        height: 210,
                        format: {
                          font: EMBEDDED_FONT_FAMILY,
                          size: 14,
                          color: 0xe2e8f0,
                          bold: false,
                          italic: false,
                          leading: 3,
                          align: 'left',
                          fieldOptions: {
                            embed_fonts: true,
                            selectable: false,
                          },
                        },
                        word_wrap: true,
                        multiline: true,
                      },
                      scripts: [],
                    },
                    {
                      id: 'as2-nested-inst',
                      name: 'as2Nested',
                      transform: { x: 40, y: 24, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'as2-nested-clip',
                        playback: 'independent',
                      },
                      scripts: [
                        {
                          event: 'load',
                          script: {
                            language: 'action_script2',
                            source: '_root.as2InstanceLoads = (_root.as2InstanceLoads ? _root.as2InstanceLoads : 0) + 1;',
                          },
                        },
                      ],
                    },
                    {
                      id: 'as3-nested-inst',
                      name: 'as3Nested',
                      transform: { x: 190, y: 24, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'as3-nested-clip',
                        playback: 'independent',
                      },
                      scripts: [],
                    },
                    {
                      id: 'action-btn-inst',
                      name: 'actionBtn',
                      transform: { x: 40, y: 240, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'action-btn-sym',
                        playback: 'independent',
                      },
                      scripts: [
                        {
                          event: 'click',
                          script: {
                            language: 'action_script2',
                            source: '_root.as2BtnClicks = (_root.as2BtnClicks ? _root.as2BtnClicks : 0) + 1;\n_root.as2BtnMouse = Math.round(_root._xmouse) + "," + Math.round(_root._ymouse);\n_root.as2Nested.gotoAndPlay(4);',
                          },
                        },
                      ],
                    },
                    {
                      id: 'net-mc-inst',
                      name: 'netMc',
                      transform: { x: 230, y: 240, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'net-mc-sym',
                        playback: 'independent',
                      },
                      scripts: [
                        {
                          event: 'click',
                          script: {
                            language: 'action_script2',
                            source: '_root.as2McClicks = (_root.as2McClicks ? _root.as2McClicks : 0) + 1;\n_root.runAs2LoadVars();',
                          },
                        },
                      ],
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
                source: rootAs2Frame0,
              },
            },
            {
              frame: 3,
              script: {
                language: 'action_script2',
                source: 'stop();\nthis.as2RootStoppedFrame = this._currentframe;',
              },
            },
          ],
        },
      },
      {
        id: 'as2-nested-clip',
        name: 'AS2 Nested Clip',
        kind: 'movie_clip',
        timeline: {
          frame_count: 6,
          layers: [
            {
              id: 'as2-nested-layer',
              name: 'Box',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: 'start',
                  elements: [rectElement('as2-box-f1', 0, 0, 120, 76, [230, 55, 65, 255])],
                },
                {
                  frame: 2,
                  tween: null,
                  label: 'boot_rest',
                  elements: [rectElement('as2-box-f3', 0, 0, 120, 76, [40, 210, 95, 255])],
                },
                {
                  frame: 3,
                  tween: null,
                  label: 'step2',
                  elements: [rectElement('as2-box-f4', 0, 0, 120, 76, [240, 195, 45, 255])],
                },
                {
                  frame: 5,
                  tween: null,
                  label: 'advanced_rest',
                  elements: [rectElement('as2-box-f6', 0, 0, 120, 76, [235, 60, 190, 255])],
                },
              ],
            },
          ],
          scripts: [
            {
              frame: 0,
              script: {
                language: 'action_script2',
                source: '_root.as2NestedVisits = (_root.as2NestedVisits ? _root.as2NestedVisits : 0) + 1;',
              },
            },
            {
              frame: 2,
              script: {
                language: 'action_script2',
                source: 'stop();\n_root.as2NestedPhase = "boot_f3";\n_root.as2NestedVisits = _root.as2NestedVisits + 1;',
              },
            },
            {
              frame: 5,
              script: {
                language: 'action_script2',
                source: 'stop();\n_root.as2NestedPhase = "advanced_f6";\n_root.as2NestedVisits = _root.as2NestedVisits + 1;',
              },
            },
          ],
        },
      },
      {
        id: 'as3-nested-clip',
        name: 'AS3 Nested Clip',
        kind: 'movie_clip',
        class_name: 'gate.As3Spinner',
        timeline: {
          frame_count: 6,
          layers: [
            {
              id: 'as3-nested-layer',
              name: 'Box',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: 'start',
                  elements: [rectElement('as3-box-f1', 0, 0, 120, 76, [145, 70, 235, 255])],
                },
                {
                  frame: 2,
                  tween: null,
                  label: 'boot_rest',
                  elements: [rectElement('as3-box-f3', 0, 0, 120, 76, [35, 205, 235, 255])],
                },
                {
                  frame: 3,
                  tween: null,
                  label: 'step2',
                  elements: [rectElement('as3-box-f4', 0, 0, 120, 76, [245, 130, 40, 255])],
                },
                {
                  frame: 5,
                  tween: null,
                  label: 'advanced_rest',
                  elements: [rectElement('as3-box-f6', 0, 0, 120, 76, [250, 210, 50, 255])],
                },
              ],
            },
          ],
          scripts: [
            {
              frame: 2,
              script: {
                language: 'action_script3',
                source: 'stop();\nthis.reachPhase("boot_f3");',
              },
            },
            {
              frame: 5,
              script: {
                language: 'action_script3',
                source: 'stop();\nthis.reachPhase("advanced_f6");',
              },
            },
          ],
        },
      },
      {
        id: 'action-btn-sym',
        name: 'Action Button',
        kind: 'button',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'action-btn-layer',
              name: 'Face',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: '',
                  elements: [
                    rectElement('action-btn-face', 0, 0, 170, 52, [38, 124, 215, 255]),
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
      {
        id: 'net-mc-sym',
        name: 'Network Trigger Clip',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'net-mc-layer',
              name: 'Face',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: '',
                  elements: [
                    rectElement('net-mc-face', 0, 0, 180, 52, [44, 150, 118, 255]),
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
    ],
    sources: [
      {
        id: 'src-spinner',
        path: 'gate/As3Spinner.as',
        language: 'action_script3',
        source: as3SpinnerSource,
      },
      {
        id: 'src-main',
        path: 'gate/Main.as',
        language: 'action_script3',
        source: as3MainSource,
      },
    ],
    resources: [
      {
        id: 'gate-font-ttf',
        path: `fonts/${EMBEDDED_FONT_FAMILY} Regular.ttf`,
        media_type: 'font/ttf',
        data_base64: assets.fontBase64,
      },
      {
        id: 'gate-font-embedding',
        path: 'fonts/embedding.json',
        media_type: 'application/json',
        data_base64: assets.fontMetadataBase64,
      },
      {
        id: 'gate-beep',
        path: 'audio/beep.wav',
        media_type: 'audio/wav',
        data_base64: assets.beepWavBase64,
      },
      {
        id: 'gate-video',
        path: 'media/motion.mp4',
        media_type: 'video/mp4',
        data_base64: assets.motionMp4Base64,
      },
      {
        id: 'gate-bundled-note',
        path: 'data/bundled-note.txt',
        media_type: 'text/plain',
        data_base64: Buffer.from('bundled-gate-ok', 'utf8').toString('base64'),
      },
      {
        id: 'gate-bundled-vars',
        path: 'data/bundled-vars.txt',
        media_type: 'text/plain',
        data_base64: Buffer.from('bundled=yes&mode=offline', 'utf8').toString('base64'),
      },
    ],
  };
}

function buildScriptlessBannerProject(assets) {
  return {
    format_version: 12,
    name: 'Firefox Compact Scriptless Banner',
    width: 640,
    height: 240,
    fps: 24,
    background: [18, 26, 40, 255],
    root: 'banner-scene',
    symbols: [
      {
        id: 'banner-scene',
        name: 'Banner Scene',
        kind: 'movie_clip',
        timeline: {
          frame_count: 12,
          layers: [
            {
              id: 'banner-sound',
              name: 'Sound',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  elements: [],
                  tween: null,
                  label: '',
                  sound: {
                    resource: 'gate-beep',
                    sync: 'start',
                  },
                },
              ],
            },
            {
              id: 'banner-art',
              name: 'Art',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: 'linear',
                  label: '',
                  elements: [
                    rectElement('banner-pill', 40, 120, 96, 64, [245, 95, 70, 255]),
                  ],
                },
                {
                  frame: 11,
                  tween: null,
                  label: '',
                  elements: [
                    rectElement('banner-pill', 460, 120, 96, 64, [55, 200, 245, 255]),
                  ],
                },
              ],
            },
            {
              id: 'banner-text-layer',
              name: 'Headline',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: '',
                  elements: [
                    {
                      id: 'banner-headline',
                      name: 'bannerHeadline',
                      transform: { x: 40, y: 32, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'text_box',
                        text: 'COMPACT BANNER',
                        width: 480,
                        height: 54,
                        format: {
                          font: EMBEDDED_FONT_FAMILY,
                          size: 32,
                          color: 0x66ff99,
                          bold: false,
                          italic: false,
                          leading: 2,
                          align: 'left',
                          fieldOptions: {
                            embed_fonts: true,
                            selectable: false,
                          },
                        },
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
    ],
    sources: [],
    resources: [
      {
        id: 'gate-font-ttf',
        path: `fonts/${EMBEDDED_FONT_FAMILY} Regular.ttf`,
        media_type: 'font/ttf',
        data_base64: assets.fontBase64,
      },
      {
        id: 'gate-font-embedding',
        path: 'fonts/embedding.json',
        media_type: 'application/json',
        data_base64: assets.fontMetadataBase64,
      },
      {
        id: 'gate-beep',
        path: 'audio/beep.wav',
        media_type: 'audio/wav',
        data_base64: assets.beepWavBase64,
      },
    ],
  };
}

// ---------------------------------------------------------------------------
// Local HTTP server (with Range 206 Partial Content for Firefox <video>)
// ---------------------------------------------------------------------------
const MIME_BY_EXT = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.mp4': 'video/mp4',
  '.wav': 'audio/wav',
  '.ttf': 'font/ttf',
  '.png': 'image/png',
};

function startStaticHttpServer(rootDir) {
  const requests = [];
  const server = http.createServer((req, res) => {
    const reqUrl = new URL(req.url || '/', 'http://127.0.0.1');
    let relPath = decodeURIComponent(reqUrl.pathname).replace(/^\/+/, '');
    if (!relPath || relPath.endsWith('/')) relPath += 'index.html';
    const fullPath = path.resolve(rootDir, relPath);
    requests.push({ method: req.method, path: reqUrl.pathname });

    if (!fullPath.startsWith(path.resolve(rootDir) + path.sep) && fullPath !== path.resolve(rootDir)) {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }
    if (reqUrl.pathname === '/favicon.ico') {
      res.writeHead(204);
      res.end();
      return;
    }
    if (!fs.existsSync(fullPath) || fs.statSync(fullPath).isDirectory()) {
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      res.end('Not found');
      return;
    }

    const stat = fs.statSync(fullPath);
    const ext = path.extname(fullPath).toLowerCase();
    const contentType = MIME_BY_EXT[ext] || 'application/octet-stream';
    const rangeHeader = req.headers.range;

    if (rangeHeader && /^bytes=\d*-\d*$/.test(rangeHeader)) {
      const [startStr, endStr] = rangeHeader.slice(6).split('-');
      const total = stat.size;
      const start = startStr ? Number(startStr) : 0;
      const end = endStr ? Math.min(Number(endStr), total - 1) : total - 1;
      if (start >= 0 && start <= end && end < total) {
        const chunk = fs.readFileSync(fullPath).subarray(start, end + 1);
        res.writeHead(206, {
          'Content-Type': contentType,
          'Accept-Ranges': 'bytes',
          'Content-Range': `bytes ${start}-${end}/${total}`,
          'Content-Length': String(chunk.length),
          'Cache-Control': 'no-store',
        });
        res.end(req.method === 'HEAD' ? undefined : chunk);
        return;
      }
    }

    const bytes = fs.readFileSync(fullPath);
    res.writeHead(200, {
      'Content-Type': contentType,
      'Accept-Ranges': 'bytes',
      'Content-Length': String(bytes.length),
      'Cache-Control': 'no-store',
    });
    res.end(req.method === 'HEAD' ? undefined : bytes);
  });

  return new Promise((resolve, reject) => {
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      resolve({
        port: addr.port,
        baseUrl: `http://127.0.0.1:${addr.port}`,
        requests,
        close: () =>
          new Promise(r => {
            server.closeAllConnections?.();
            server.close(() => r());
          }),
      });
    });
  });
}

// ---------------------------------------------------------------------------
// Native Firefox WebDriver BiDi session
// ---------------------------------------------------------------------------
const PRELOAD_MONITOR_BODY = `(() => {
  if (window.__nfGateMonitor) return;
  const activeRafs = new Map();
  const activeIntervals = new Set();
  const audioContexts = [];
  const pageErrors = [];
  const rejections = [];
  const consoleErrors = [];

  const rawRaf = window.requestAnimationFrame;
  const rawCaf = window.cancelAnimationFrame;
  if (typeof rawRaf === 'function' && typeof rawCaf === 'function') {
    window.requestAnimationFrame = function(cb) {
      const stack = String(new Error().stack || '');
      let id = 0;
      id = rawRaf.call(window, function(ts) {
        activeRafs.delete(id);
        return cb(ts);
      });
      activeRafs.set(id, id + ':' + stack.split('\\n').slice(0, 3).join(' | '));
      return id;
    };
    window.cancelAnimationFrame = function(id) {
      activeRafs.delete(id);
      return rawCaf.call(window, id);
    };
  }

  const rawSetInterval = window.setInterval;
  const rawClearInterval = window.clearInterval;
  if (typeof rawSetInterval === 'function' && typeof rawClearInterval === 'function') {
    window.setInterval = function(...args) {
      const id = rawSetInterval.apply(window, args);
      activeIntervals.add(id);
      return id;
    };
    window.clearInterval = function(id) {
      activeIntervals.delete(id);
      return rawClearInterval.call(window, id);
    };
  }

  const OrigAudioCtx = window.AudioContext || window.webkitAudioContext;
  if (typeof OrigAudioCtx === 'function') {
    const WrappedAudioCtx = function(...args) {
      const ctx = new OrigAudioCtx(...args);
      audioContexts.push(ctx);
      return ctx;
    };
    WrappedAudioCtx.prototype = OrigAudioCtx.prototype;
    Object.setPrototypeOf(WrappedAudioCtx, OrigAudioCtx);
    window.AudioContext = WrappedAudioCtx;
  }

  const rawConsoleError = console.error;
  if (typeof rawConsoleError === 'function') {
    console.error = function(...args) {
      consoleErrors.push(args.map(a => (a instanceof Error ? (a.stack || a.message) : String(a))).join(' '));
      return rawConsoleError.apply(console, args);
    };
  }

  window.addEventListener('error', (ev) => {
    pageErrors.push(String(ev?.error?.stack || ev?.message || 'window error'));
  });
  window.addEventListener('unhandledrejection', (ev) => {
    const r = ev?.reason;
    rejections.push(String(r?.stack || r?.message || r || 'unhandledrejection'));
  });

  window.__nfGateMonitor = {
    restoreNativeHooks() {
      if (typeof rawRaf === 'function') window.requestAnimationFrame = rawRaf;
      if (typeof rawCaf === 'function') window.cancelAnimationFrame = rawCaf;
      if (typeof rawSetInterval === 'function') window.setInterval = rawSetInterval;
      if (typeof rawClearInterval === 'function') window.clearInterval = rawClearInterval;
      if (typeof OrigAudioCtx === 'function') window.AudioContext = OrigAudioCtx;
      if (typeof rawConsoleError === 'function') console.error = rawConsoleError;
    },
    get activeRafCount() { return activeRafs.size; },
    get activeRafStacks() { return Array.from(activeRafs.values()); },
    get activeIntervalCount() { return activeIntervals.size; },
    get audioStates() { return audioContexts.map(c => c.state); },
    get pageErrors() { return pageErrors.slice(); },
    get rejections() { return rejections.slice(); },
    get consoleErrors() { return consoleErrors.slice(); },
  };
})()`;

async function launchFirefoxBidi(firefoxBin) {
  const profileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-firefox-gate-'));
  // Intentionally preserve Firefox's default `security.fileuri.strict_origin_policy = true`
  // so every `file://` scenario runs under genuine Firefox file origin isolation.
  // `media.cubeb.force_mock_context = true` uses Gecko's built-in MockCubeb software audio
  // clock so AudioContext.resume() works on headless CI VMs with zero CoreAudio/ALSA hardware
  // devices while still enforcing Firefox's real user-gesture autoplay policy.
  const prefs = [
    'user_pref("remote.active-protocols", 1);',
    'user_pref("browser.shell.checkDefaultBrowser", false);',
    'user_pref("browser.startup.homepage_override.mstone", "ignore");',
    'user_pref("browser.tabs.warnOnClose", false);',
    'user_pref("datareporting.policy.dataSubmissionEnabled", false);',
    'user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);',
    'user_pref("dom.disable_open_during_load", false);',
    'user_pref("media.cubeb.force_mock_context", true);',
    'user_pref("no-remote", true);',
  ].join('\n');
  fs.writeFileSync(path.join(profileDir, 'user.js'), prefs, 'utf8');

  const proc = spawn(
    firefoxBin,
    ['--headless', '--no-remote', '-profile', profileDir, '--remote-debugging-port=0', 'about:blank'],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  );

  let wsBase = null;
  let stderrTail = '';
  const parseEndpoint = chunk => {
    const text = chunk.toString('utf8');
    stderrTail = (stderrTail + text).slice(-4096);
    const m = text.match(/WebDriver BiDi listening on (ws:\/\/127\.0\.0\.1:\d+)/);
    if (m) wsBase = m[1];
  };
  proc.stderr.on('data', parseEndpoint);
  proc.stdout.on('data', parseEndpoint);

  const cleanup = () => {
    try {
      proc.stdout?.destroy();
      proc.stderr?.destroy();
    } catch (_) {}
    try {
      if (proc.exitCode === null) proc.kill('SIGKILL');
    } catch (_) {}
    try {
      fs.rmSync(profileDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    } catch (_) {}
  };

  const onSignal = () => {
    cleanup();
    process.exit(130);
  };
  process.once('SIGINT', onSignal);
  process.once('SIGTERM', onSignal);

  try {
    const deadline = Date.now() + 15000;
    while (!wsBase && Date.now() < deadline) {
      if (proc.exitCode !== null) {
        throw new Error(`Firefox exited early with code ${proc.exitCode}:\n${stderrTail}`);
      }
      const activePortFile = path.join(profileDir, 'DevToolsActivePort');
      if (fs.existsSync(activePortFile)) {
        const lines = fs.readFileSync(activePortFile, 'utf8').trim().split(/\r?\n/);
        if (lines[0] && /^\d+$/.test(lines[0])) {
          wsBase = `ws://127.0.0.1:${lines[0]}`;
          break;
        }
      }
      await new Promise(r => setTimeout(r, 40));
    }
    if (!wsBase) {
      throw new Error(`Timed out waiting for Firefox WebDriver BiDi endpoint.\n${stderrTail}`);
    }

    const ws = new WebSocket(`${wsBase}/session`);
    await new Promise((resolve, reject) => {
      const openTimer = setTimeout(() => {
        reject(new Error('Timed out opening Firefox BiDi WebSocket session'));
      }, 10000);
      openTimer.unref?.();
      ws.onopen = () => {
        clearTimeout(openTimer);
        resolve();
      };
      ws.onerror = err => {
        clearTimeout(openTimer);
        reject(new Error(`BiDi WebSocket connection failed: ${err?.message || err}`));
      };
    });

    let nextId = 1;
    let wsClosed = false;
    const pending = new Map();
    const logEntries = [];
    const networkFailures = [];
    const unloadProofs = [];

    const rejectAllPending = reason => {
      wsClosed = true;
      for (const [, entry] of pending) {
        clearTimeout(entry.timer);
        entry.reject(new Error(reason));
      }
      pending.clear();
    };
    ws.onclose = () => rejectAllPending('Firefox BiDi WebSocket closed');
    ws.onerror = err => rejectAllPending(`Firefox BiDi WebSocket error: ${err?.message || err}`);

    ws.onmessage = ev => {
      const msg = JSON.parse(String(ev.data));
      if (msg.id && pending.has(msg.id)) {
        const entry = pending.get(msg.id);
        pending.delete(msg.id);
        clearTimeout(entry.timer);
        if (msg.type === 'error') {
          entry.reject(new Error(`BiDi ${entry.method} failed: ${msg.error} — ${msg.message}`));
        } else {
          entry.resolve(msg.result);
        }
        return;
      }
      if (msg.type === 'event') {
        if (msg.method === 'log.entryAdded') {
          const p = msg.params;
          if (typeof p?.text === 'string' && p.text.startsWith('[NF_UNLOAD_PROOF] ')) {
            try {
              unloadProofs.push(JSON.parse(p.text.slice('[NF_UNLOAD_PROOF] '.length)));
            } catch (_) {}
          }
          logEntries.push(p);
        } else if (msg.method === 'network.responseCompleted') {
          const status = msg.params?.response?.status || 0;
          const url = msg.params?.request?.url || '';
          if (status >= 400 && !url.endsWith('/favicon.ico')) {
            networkFailures.push({ url, status });
          }
        } else if (msg.method === 'network.fetchError') {
          const url = msg.params?.request?.url || '';
          if (!url.endsWith('/favicon.ico')) {
            networkFailures.push({ url, errorText: msg.params?.errorText || 'fetchError' });
          }
        }
      }
    };

    const send = (method, params = {}, timeoutMs = 30000) =>
      new Promise((resolve, reject) => {
        if (wsClosed || ws.readyState !== 1) {
          reject(new Error(`BiDi ${method} aborted: WebSocket is not open`));
          return;
        }
        const id = nextId++;
        const timer = setTimeout(() => {
          if (pending.delete(id)) {
            reject(new Error(`BiDi ${method} timed out after ${timeoutMs}ms`));
          }
        }, timeoutMs);
        timer.unref?.();
        pending.set(id, { method, resolve, reject, timer });
        try {
          ws.send(JSON.stringify({ id, method, params }));
        } catch (err) {
          clearTimeout(timer);
          pending.delete(id);
          reject(err);
        }
      });

    await send('session.new', {
      capabilities: { alwaysMatch: { acceptInsecureCerts: true } },
    });
    await send('session.subscribe', {
      events: [
        'log.entryAdded',
        'browsingContext.load',
        'network.responseCompleted',
        'network.fetchError',
      ],
    });
    const { context } = await send('browsingContext.create', { type: 'tab' });
    await send('browsingContext.setViewport', {
      context,
      viewport: { width: 1280, height: 900 },
    });
    const evalJson = async fnDeclaration => {
      const res = await send('script.callFunction', {
        target: { context },
        functionDeclaration: fnDeclaration,
        awaitPromise: true,
      });
      if (res?.type === 'exception') {
        throw new Error(`Page evaluation threw: ${JSON.stringify(res.exceptionDetails)}`);
      }
      const raw = res?.result?.value;
      return typeof raw === 'string' ? JSON.parse(raw) : raw;
    };

    const installContentMonitor = async () => {
      // Pass a primitive string to `window.setTimeout(..., 0)` so Gecko compiles and executes
      // the monitor on a fresh event-loop turn in the page's real content realm rather than
      // inheriting WebDriver BiDi's ExpandedPrincipal sandbox (which Gecko nukes before pagehide).
      await evalJson(`() => {
        window.setTimeout(${JSON.stringify(PRELOAD_MONITOR_BODY)}, 0);
        return JSON.stringify({ scheduled: true });
      }`);
      await waitFor('content monitor installation', async () => {
        return evalJson(`() => JSON.stringify(Boolean(window.__nfGateMonitor))`);
      });
    };

    const waitFor = async (description, predicateFn, timeoutMs = 8000) => {
      const start = Date.now();
      let lastVal = null;
      while (Date.now() - start < timeoutMs) {
        lastVal = await predicateFn();
        if (lastVal) return lastVal;
        await new Promise(r => setTimeout(r, 45));
      }
      throw new Error(`Timed out waiting for ${description} (last=${JSON.stringify(lastVal)})`);
    };

    const clickClientPoint = async (x, y) => {
      await send('input.performActions', {
        context,
        actions: [
          {
            type: 'pointer',
            id: 'mouse',
            parameters: { pointerType: 'mouse' },
            actions: [
              { type: 'pointerMove', x: Math.round(x), y: Math.round(y) },
              { type: 'pointerDown', button: 0 },
              { type: 'pointerUp', button: 0 },
            ],
          },
        ],
      });
    };

    const hoverClientPoint = async (x, y) => {
      await send('input.performActions', {
        context,
        actions: [
          {
            type: 'pointer',
            id: 'mouse',
            parameters: { pointerType: 'mouse' },
            actions: [{ type: 'pointerMove', x: Math.round(x), y: Math.round(y) }],
          },
        ],
      });
    };

    const sendKeyActions = async keyActions => {
      await send('input.performActions', {
        context,
        actions: [{ type: 'key', id: 'keyboard', actions: keyActions }],
      });
    };

    const capturePng = async () => {
      const shot = await send('browsingContext.captureScreenshot', { context });
      return decodePngRgba(Buffer.from(shot.data, 'base64'));
    };

    const resetScenarioLogs = () => {
      logEntries.length = 0;
      networkFailures.length = 0;
      unloadProofs.length = 0;
    };

    const collectHardErrors = async () => {
      const bidiErrors = logEntries
        .filter(e => e.level === 'error')
        .map(e => `[BiDi:${e.type}] ${e.text}`);
      let inPage = { pageErrors: [], rejections: [], consoleErrors: [] };
      try {
        inPage = await evalJson(`() => window.eval('JSON.stringify(window.__nfGateMonitor ? { pageErrors: window.__nfGateMonitor.pageErrors, rejections: window.__nfGateMonitor.rejections, consoleErrors: window.__nfGateMonitor.consoleErrors } : { pageErrors: [], rejections: [], consoleErrors: [] })')`);
      } catch (_) {}
      return {
        bidiErrors,
        pageErrors: inPage.pageErrors || [],
        rejections: inPage.rejections || [],
        consoleErrors: inPage.consoleErrors || [],
        networkFailures: networkFailures.slice(),
      };
    };

    const close = async () => {
      process.removeListener('SIGINT', onSignal);
      process.removeListener('SIGTERM', onSignal);
      try {
        await send('session.end', {}, 2000);
      } catch (_) {}
      try {
        ws.close();
      } catch (_) {}
      if (proc.exitCode === null) {
        const exited = new Promise(resolve => proc.once('exit', resolve));
        try {
          proc.kill('SIGKILL');
        } catch (_) {}
        await Promise.race([
          exited,
          new Promise(resolve => {
            const t = setTimeout(resolve, 1500);
            t.unref?.();
          }),
        ]);
      }
      cleanup();
    };

    return {
      context,
      send,
      evalJson,
      installContentMonitor,
      waitFor,
      clickClientPoint,
      hoverClientPoint,
      sendKeyActions,
      capturePng,
      resetScenarioLogs,
      collectHardErrors,
      unloadProofs,
      close,
    };
  } catch (err) {
    process.removeListener('SIGINT', onSignal);
    process.removeListener('SIGTERM', onSignal);
    cleanup();
    throw err;
  }
}

// ---------------------------------------------------------------------------
// Scenario assertions
// ---------------------------------------------------------------------------
const KEY_ARROW_RIGHT = '\uE014';
const KEY_ARROW_LEFT = '\uE012';
const KEY_SHIFT = '\uE008';

function assertZeroErrors(label, errs) {
  assert.deepEqual(errs.bidiErrors, [], `${label}: expected 0 BiDi console/JS errors`);
  assert.deepEqual(errs.consoleErrors, [], `${label}: expected 0 console.error calls`);
  assert.deepEqual(errs.pageErrors, [], `${label}: expected 0 uncaught window error events`);
  assert.deepEqual(errs.rejections, [], `${label}: expected 0 unhandled promise rejections`);
  assert.deepEqual(errs.networkFailures, [], `${label}: expected 0 HTTP/network failures`);
}

async function verifyUnloadLifecycle(bidi, label, renderMode, blankUrl) {
  bidi.unloadProofs.length = 0;
  // Opt out of BFCache (via beforeunload/unload listeners + Web Lock when available) so Firefox
  // performs a full document teardown (`event.persisted === false`) when navigating away, and
  // restore native WebIDL hooks before unload so Gecko's BiDi sandbox teardown cannot invalidate
  // wrapped functions during `pagehide`.
  const unloadObserverCode = `
    (() => {
      const mon = window.__nfGateMonitor;
      const preUnloadRafCount = mon ? mon.activeRafCount : -1;
      const preUnloadIntervalCount = mon ? mon.activeIntervalCount : -1;
      const preAudioStates = mon ? mon.audioStates : [];
      const preAudioToggleHidden = Boolean(document.getElementById('audio-toggle')?.hidden);
      if (mon && typeof mon.restoreNativeHooks === 'function') {
        mon.restoreNativeHooks();
      }
      let pagehidePersisted = false;
      window.addEventListener('beforeunload', () => {});
      window.addEventListener('pagehide', (ev) => {
        pagehidePersisted = Boolean(ev.persisted);
      });
      window.addEventListener('unload', () => {
        const domRoot = document.getElementById('stage-dom-root');
        console.info('[NF_UNLOAD_PROOF] ' + JSON.stringify({
          persisted: pagehidePersisted,
          preUnloadRafCount,
          preUnloadIntervalCount,
          preAudioContextCount: preAudioStates.length,
          preAudioToggleHidden,
          postAudioToggleHidden: Boolean(document.getElementById('audio-toggle')?.hidden),
          postAudioStatusHidden: Boolean(document.getElementById('audio-status')?.hidden),
          activeTextOverlays: document.querySelectorAll('[data-neoflash-text-input]').length,
          pageErrors: mon ? mon.pageErrors : [],
          consoleErrors: mon ? mon.consoleErrors : [],
          rejections: mon ? mon.rejections : [],
          domDisposeCount: domRoot ? domRoot.disposeCount : null,
          domMounted: domRoot ? domRoot.isMounted : null,
          domChildren: domRoot ? domRoot.children.length : null,
        }));
      });
      if (navigator.locks && typeof navigator.locks.request === 'function') {
        navigator.locks.request('nf_firefox_gate_unload_' + Math.random(), () => {
          window.__nfUnloadObserverReady = true;
          return new Promise(() => {});
        }).catch(() => {
          window.__nfUnloadObserverReady = true;
        });
      } else {
        window.__nfUnloadObserverReady = true;
      }
    })();
  `;
  await bidi.evalJson(`() => {
    window.__nfUnloadObserverReady = false;
    window.setTimeout(${JSON.stringify(unloadObserverCode)}, 0);
    return JSON.stringify({ scheduled: true });
  }`);
  await bidi.waitFor(`${label} unload observer arm`, async () => {
    return bidi.evalJson(`() => JSON.stringify(Boolean(window.__nfUnloadObserverReady))`);
  });

  await bidi.send('browsingContext.navigate', {
    context: bidi.context,
    url: blankUrl,
    wait: 'complete',
  });

  const proof = await bidi.waitFor(
    `${label} unload proof`,
    async () => (bidi.unloadProofs.length > 0 ? bidi.unloadProofs[bidi.unloadProofs.length - 1] : null),
    4000,
  );

  if (proof.setupError) {
    throw new Error(`${label}: unload setup error: ${proof.setupError}`);
  }
  assert.equal(proof.persisted, false, `${label}: pagehide must execute full non-BFCache teardown`);
  assert.deepEqual(proof.pageErrors || [], [], `${label}: 0 window errors during pagehide/unload`);
  assert.deepEqual(proof.consoleErrors || [], [], `${label}: 0 console errors during pagehide/unload`);
  assert.deepEqual(proof.rejections || [], [], `${label}: 0 unhandled rejections during pagehide/unload`);
  assert.ok(
    proof.preUnloadRafCount >= 1,
    `${label}: animation loop must be active prior to unload (proof: ${JSON.stringify(proof)})`,
  );
  assert.equal(proof.preUnloadIntervalCount, 0, `${label}: expected 0 leaked intervals prior to unload`);
  assert.ok(proof.preAudioContextCount >= 1, `${label}: at least one AudioContext must be created`);
  assert.equal(proof.preAudioToggleHidden, false, `${label}: audio HUD must be active prior to unload`);
  assert.equal(
    proof.postAudioToggleHidden,
    true,
    `${label}: pagehide teardown must dispose audio/video hosts back to idle (proof: ${JSON.stringify(proof)})`,
  );
  assert.equal(proof.postAudioStatusHidden, true, `${label}: pagehide teardown must hide audio status`);
  assert.equal(proof.activeTextOverlays, 0, `${label}: pagehide teardown must remove active text overlays`);
  if (renderMode === 'dom') {
    assert.equal(proof.domDisposeCount, 1, `${label}: DOM stage must dispose once on unload`);
    assert.equal(proof.domMounted, false, `${label}: DOM stage must mark unmounted on unload`);
    assert.equal(proof.domChildren, 0, `${label}: DOM stage must remove all scene children on unload`);
  }

  const postUnloadErrors = await bidi.collectHardErrors();
  assertZeroErrors(`${label} (post-unload)`, postUnloadErrors);
}

async function runSuiteScenario(bidi, spec) {
  const { id, url, blankUrl, protocol, sizeLevel, renderMode } = spec;
  bidi.resetScenarioLogs();

  await bidi.send('browsingContext.navigate', {
    context: bidi.context,
    url,
    wait: 'complete',
  });
  await bidi.installContentMonitor();

  // 1. Wait for WASM player boot, root timeline stop at frame 4, and both AS2 + AS3 nested clips
  //    reaching their boot resting frame 3 (`boot_f3`).
  const bootState = await bidi.waitFor(`${id} boot + nested timelines`, async () => {
    const snap = await bidi.evalJson(`() => {
      const errEl = document.getElementById('error');
      if (errEl && errEl.style.display === 'block') {
        return JSON.stringify({ bootError: errEl.textContent });
      }
      const root = window.NeoFlashPlayer?.root;
      if (!root || typeof root.getGateSnapshot !== 'function') return JSON.stringify(null);
      const gate = root.getGateSnapshot();
      if (
        gate.currentFrame < 4 ||
        gate.as2NestedPhase !== 'boot_f3' ||
        gate.spinnerPhase !== 'boot_f3' ||
        gate.bundledUrlStatus !== 'ok:bundled-gate-ok' ||
        gate.as2BundledVars !== 'yes:offline'
      ) {
        return JSON.stringify(null);
      }
      const surfaceEl = ${renderMode === 'dom' ? "document.getElementById('stage-dom-root')" : "document.getElementById('stage')"};
      const rect = surfaceEl.getBoundingClientRect();
      return JSON.stringify({
        origin: window.origin,
        userAgent: navigator.userAgent,
        fontLoaded: document.fonts.check('32px "${EMBEDDED_FONT_FAMILY}"'),
        audioBefore: document.getElementById('audio-status')?.textContent || '',
        rect: { left: rect.left, top: rect.top, width: rect.width, height: rect.height },
        gate,
      });
    }`);
    if (snap?.bootError) {
      throw new Error(`${id} failed to boot:\n${snap.bootError}`);
    }
    return snap;
  });

  assert.match(bootState.userAgent, /Firefox\//, `${id}: must execute in real Firefox`);
  if (protocol === 'file') {
    assert.equal(
      bootState.origin,
      'null',
      `${id}: file:// scenario must preserve Firefox default strict origin isolation`,
    );
  } else {
    assert.match(bootState.origin, /^http:\/\/127\.0\.0\.1:\d+$/, `${id}: HTTP origin expected`);
  }

  // Verify embedded @font-face loaded in Firefox
  assert.equal(bootState.fontLoaded, true, `${id}: embedded @font-face ${EMBEDDED_FONT_FAMILY} must load`);
  assert.equal(bootState.gate.as2InstanceLoads, 1, `${id}: AS2 instance load script must run once`);
  assert.equal(bootState.gate.as2RootStoppedFrame, 4, `${id}: AS2 root timeline must advance to frame 4`);
  assert.ok(bootState.gate.spinnerEvents >= 1, `${id}: AS3 bubbling spinnerPhase event must dispatch`);

  const stageToClient = (sx, sy) => ({
    x: bootState.rect.left + (sx / 800) * bootState.rect.width,
    y: bootState.rect.top + (sy / 600) * bootState.rect.height,
  });

  // 2. Verify initial Stage pixels (non-blank embedded font glyphs + frame-3 nested clip colors:
  //    AS2 nested at (40,24,120,76) = Green [40,210,95], AS3 nested at (190,24,120,76) = Cyan [35,205,235]).
  if (renderMode === 'canvas') {
    const canvasCheck = await bidi.evalJson(`() => {
      const ctx = document.getElementById('stage').getContext('2d');
      const pAs2 = Array.from(ctx.getImageData(100, 60, 1, 1).data);
      const pAs3 = Array.from(ctx.getImageData(250, 60, 1, 1).data);
      const fontData = ctx.getImageData(374, 38, 300, 48).data;
      let fontGlyphPixels = 0;
      for (let i = 0; i < fontData.length; i += 4) {
        if (fontData[i + 1] > 180 && fontData[i] < 140) fontGlyphPixels++;
      }
      return JSON.stringify({ pAs2, pAs3, fontGlyphPixels });
    }`);
    assert.ok(
      canvasCheck.pAs2[1] > 180 && canvasCheck.pAs2[0] < 80,
      `${id}: AS2 nested clip frame 3 pixel must be green, got ${canvasCheck.pAs2}`,
    );
    assert.ok(
      canvasCheck.pAs3[1] > 170 && canvasCheck.pAs3[2] > 200 && canvasCheck.pAs3[0] < 80,
      `${id}: AS3 nested clip frame 3 pixel must be cyan, got ${canvasCheck.pAs3}`,
    );
    assert.ok(
      canvasCheck.fontGlyphPixels >= 120,
      `${id}: Canvas embedded font headline must render non-blank lime glyph pixels (${canvasCheck.fontGlyphPixels})`,
    );
  } else {
    const domCheck = await bidi.evalJson(`() => {
      const texts = Array.from(document.querySelectorAll('.nf-dom-text'));
      const headline = texts.find(el => el.textContent.includes('GATE FONT 88'));
      const computedFont = headline ? getComputedStyle(headline).fontFamily : '';
      return JSON.stringify({
        svgCount: document.querySelectorAll('.nf-dom-svg').length,
        buttonCount: document.querySelectorAll('.nf-dom-button').length,
        inputCount: document.querySelectorAll('.nf-dom-input').length,
        computedFont,
      });
    }`);
    assert.ok(domCheck.svgCount >= 4, `${id}: DOM mode must mount SVG shape leaves`);
    assert.ok(domCheck.buttonCount >= 2, `${id}: DOM mode must mount interactive button overlays`);
    assert.equal(domCheck.inputCount, 1, `${id}: DOM mode must mount native input control`);
    assert.match(domCheck.computedFont, /NeoFlashGateEmbedded/, `${id}: DOM headline must use embedded font`);
  }

  // Also verify real Gecko compositor screenshot pixels for BOTH Canvas and DOM modes!
  const bootPng = await bidi.capturePng();
  const as2ScreenPt = stageToClient(100, 60);
  const as3ScreenPt = stageToClient(250, 60);
  const fontScreenRect = {
    ...stageToClient(374, 38),
    width: (300 / 800) * bootState.rect.width,
    height: (48 / 600) * bootState.rect.height,
  };
  const as2ShotBoot = samplePngRegion(
    bootPng,
    { x: as2ScreenPt.x - 4, y: as2ScreenPt.y - 4, width: 8, height: 8 },
    (r, g, b) => g > 175 && r < 85 && b < 130,
  );
  const as3ShotBoot = samplePngRegion(
    bootPng,
    { x: as3ScreenPt.x - 4, y: as3ScreenPt.y - 4, width: 8, height: 8 },
    (r, g, b) => g > 170 && b > 195 && r < 85,
  );
  const fontShotBoot = samplePngRegion(bootPng, fontScreenRect, (r, g) => g > 175 && r < 140);
  assert.ok(as2ShotBoot.matched > 0, `${id}: Gecko screenshot must show green AS2 nested clip on frame 3`);
  assert.ok(as3ShotBoot.matched > 0, `${id}: Gecko screenshot must show cyan AS3 nested clip on frame 3`);
  assert.ok(
    fontShotBoot.matched >= 80,
    `${id}: Gecko screenshot must show rendered embedded font glyphs (${fontShotBoot.matched})`,
  );

  // 3. Text focus & input + selection replacement via real BiDi pointer click + keyboard actions
  const inputClickPt = stageToClient(160, 169);
  await bidi.clickClientPoint(inputClickPt.x, inputClickPt.y);

  await bidi.waitFor(`${id} input focus`, async () => {
    const f = await bidi.evalJson(`() => {
      const root = window.NeoFlashPlayer.root;
      const g = root.getGateSnapshot();
      const activeTag = document.activeElement?.tagName || '';
      return JSON.stringify(g.focusIns >= 1 && activeTag === 'INPUT' ? g : null);
    }`);
    return f;
  });

  // Normalize caret to index 0 of "ABCD", select first 2 characters ("AB") with Shift+ArrowRight x2,
  // verify both DOM and WASM selection (0..2), and type "Firefox" -> resulting text must be "FirefoxCD"!
  await bidi.sendKeyActions([
    { type: 'keyDown', value: KEY_ARROW_LEFT },
    { type: 'keyUp', value: KEY_ARROW_LEFT },
    { type: 'keyDown', value: KEY_ARROW_LEFT },
    { type: 'keyUp', value: KEY_ARROW_LEFT },
    { type: 'keyDown', value: KEY_ARROW_LEFT },
    { type: 'keyUp', value: KEY_ARROW_LEFT },
    { type: 'keyDown', value: KEY_ARROW_LEFT },
    { type: 'keyUp', value: KEY_ARROW_LEFT },
  ]);

  await bidi.waitFor(`${id} caret at 0`, async () => {
    const c = await bidi.evalJson(`() => {
      const el = document.activeElement;
      const g = window.NeoFlashPlayer.root.getGateSnapshot();
      if (!el || el.tagName !== 'INPUT') return JSON.stringify(null);
      return JSON.stringify(el.selectionStart === 0 && el.selectionEnd === 0 && g.selBegin === 0 && g.selEnd === 0 ? true : null);
    }`);
    return c;
  });

  await bidi.sendKeyActions([
    { type: 'keyDown', value: KEY_SHIFT },
    { type: 'keyDown', value: KEY_ARROW_RIGHT },
    { type: 'keyUp', value: KEY_ARROW_RIGHT },
    { type: 'keyDown', value: KEY_ARROW_RIGHT },
    { type: 'keyUp', value: KEY_ARROW_RIGHT },
    { type: 'keyUp', value: KEY_SHIFT },
  ]);

  const selectedSnap = await bidi.waitFor(`${id} text selection 0..2`, async () => {
    const sel = await bidi.evalJson(`() => {
      const el = document.activeElement;
      const g = window.NeoFlashPlayer.root.getGateSnapshot();
      if (!el || el.tagName !== 'INPUT') return JSON.stringify(null);
      return JSON.stringify(
        el.selectionStart === 0 && el.selectionEnd === 2 && g.selBegin === 0 && g.selEnd === 2
          ? { domBegin: el.selectionStart, domEnd: el.selectionEnd, value: el.value, g }
          : null
      );
    }`);
    return sel;
  });
  assert.equal(selectedSnap.domBegin, 0, `${id}: active input selectionStart must be 0`);
  assert.equal(selectedSnap.domEnd, 2, `${id}: active input selectionEnd must be 2`);
  assert.equal(selectedSnap.g.selBegin, 0, `${id}: WASM selectionBeginIndex must sync to 0`);
  assert.equal(selectedSnap.g.selEnd, 2, `${id}: WASM selectionEndIndex must sync to 2`);

  const typedActions = [];
  for (const ch of 'Firefox') {
    typedActions.push({ type: 'keyDown', value: ch }, { type: 'keyUp', value: ch });
  }
  await bidi.sendKeyActions(typedActions);

  const afterType = await bidi.waitFor(`${id} typed text FirefoxCD`, async () => {
    const g = await bidi.evalJson(`() => JSON.stringify(window.NeoFlashPlayer.root.getGateSnapshot())`);
    return g.inputText === 'FirefoxCD' ? g : null;
  });
  assert.equal(afterType.inputText, 'FirefoxCD', `${id}: WASM TextField must hold replaced text`);
  assert.equal(afterType.selBegin, 7, `${id}: WASM selectionBeginIndex after replacing 0..2 must be 7`);
  assert.equal(afterType.selEnd, 7, `${id}: WASM selectionEndIndex after replacing 0..2 must be 7`);
  assert.ok(afterType.textInputs >= 1, `${id}: TextEvent.TEXT_INPUT must fire`);
  assert.ok(afterType.textChanges >= 1, `${id}: Event.CHANGE must fire`);

  // 4. Audio unlock verification (unlocked by our real BiDi pointer/key gesture!)
  const audioUnlocked = await bidi.waitFor(`${id} Firefox AudioContext unlock`, async () => {
    const info = await bidi.evalJson(`() => window.eval(\`(() => {
      const audioText = document.getElementById('audio-status')?.textContent || '';
      const states = window.__nfGateMonitor?.audioStates || [];
      const unlocked = !audioText.includes('waiting for a gesture') && (states.includes('running') || audioText.includes('Audio playing') || audioText.includes('Audio ready'));
      return JSON.stringify(unlocked ? { audioText, states } : null);
    })()\`)`);
    return info;
  });
  assert.ok(
    audioUnlocked.states.includes('running') || audioUnlocked.audioText.startsWith('Audio '),
    `${id}: Firefox AudioContext must unlock on real user gesture`,
  );

  // 5. Pointer hover + click on interactive Button (`actionBtn` at stage 40,240 size 170x52 -> center 125,266)
  //    This also advances `as2Nested` & `as3Nested` to frame 6 (Magenta & Gold) and starts `media/motion.mp4`!
  const btnStageX = 125;
  const btnStageY = 266;
  const btnClient = stageToClient(btnStageX, btnStageY);
  await bidi.hoverClientPoint(btnClient.x, btnClient.y);
  await bidi.clickClientPoint(btnClient.x, btnClient.y);

  const afterBtn = await bidi.waitFor(`${id} actionBtn click + frame 6 + video decode`, async () => {
    const snap = await bidi.evalJson(`() => {
      const g = window.NeoFlashPlayer.root.getGateSnapshot();
      if (
        g.btnClicks < 1 ||
        g.as2BtnClicks < 1 ||
        g.as2NestedPhase !== 'advanced_f6' ||
        g.spinnerPhase !== 'advanced_f6' ||
        g.videoMetaCount < 1
      ) {
        return JSON.stringify(null);
      }
      let videoNonPanelPixels = 0;
      if (${renderMode === 'canvas'}) {
        const ctx = document.getElementById('stage').getContext('2d');
        const data = ctx.getImageData(460, 340, 200, 120).data;
        for (let i = 0; i < data.length; i += 4) {
          if (Math.abs(data[i] - 23) > 8 || Math.abs(data[i + 1] - 38) > 8 || Math.abs(data[i + 2] - 56) > 8) {
            videoNonPanelPixels++;
          }
        }
      } else {
        const vc = document.querySelector('.nf-dom-video');
        if (vc) {
          const data = vc.getContext('2d').getImageData(20, 20, 200, 120).data;
          for (let i = 0; i < data.length; i += 4) {
            if (data[i + 3] > 0 && (data[i] > 10 || data[i + 1] > 10 || data[i + 2] > 10)) {
              videoNonPanelPixels++;
            }
          }
        }
      }
      return JSON.stringify(videoNonPanelPixels > 500 ? { g, videoNonPanelPixels } : null);
    }`);
    return snap;
  }, 10000);

  assert.equal(afterBtn.g.btnClicks, 1, `${id}: Button AS3 CLICK must fire once`);
  assert.equal(afterBtn.g.as2BtnClicks, 1, `${id}: Button AS2 click must fire once`);
  assert.ok(afterBtn.g.btnRollOvers >= 1, `${id}: Button AS3 ROLL_OVER must fire`);

  const [bx, by] = afterBtn.g.btnClickCoords.split(',').map(Number);
  assert.ok(
    Math.abs(bx - btnStageX) <= 3 && Math.abs(by - btnStageY) <= 3,
    `${id}: Button stageX/stageY expected ~${btnStageX},${btnStageY}, got ${afterBtn.g.btnClickCoords}`,
  );
  const [as2Bx, as2By] = afterBtn.g.as2BtnMouse.split(',').map(Number);
  assert.ok(
    Math.abs(as2Bx - btnStageX) <= 3 && Math.abs(as2By - btnStageY) <= 3,
    `${id}: AS2 _xmouse/_ymouse expected ~${btnStageX},${btnStageY}, got ${afterBtn.g.as2BtnMouse}`,
  );
  assert.ok(afterBtn.g.videoDuration > 0, `${id}: Video metadata duration must be > 0`);
  assert.ok(afterBtn.videoNonPanelPixels > 500, `${id}: Video must decode non-zero pixels without taint`);

  // Verify nested MovieClip pixel changes on frame 6 (AS2 -> Magenta [235,60,190], AS3 -> Gold [250,210,50])
  // AND verify rendered Stage pixels of edited input text ("FirefoxCD") after blur!
  const postClickPng = await bidi.capturePng();
  const as2ShotF6 = samplePngRegion(
    postClickPng,
    { x: as2ScreenPt.x - 4, y: as2ScreenPt.y - 4, width: 8, height: 8 },
    (r, g, b) => r > 195 && b > 145 && g < 100,
  );
  const as3ShotF6 = samplePngRegion(
    postClickPng,
    { x: as3ScreenPt.x - 4, y: as3ScreenPt.y - 4, width: 8, height: 8 },
    (r, g, b) => r > 210 && g > 170 && b < 90,
  );
  const inputScreenRect = {
    ...stageToClient(44, 152),
    width: (220 / 800) * bootState.rect.width,
    height: (34 / 600) * bootState.rect.height,
  };
  const inputInkPixels = samplePngRegion(
    postClickPng,
    inputScreenRect,
    (r, g, b) => r < 90 && g < 100 && b < 120,
  );
  const videoScreenRect = {
    ...stageToClient(460, 340),
    width: (240 / 800) * bootState.rect.width,
    height: (130 / 600) * bootState.rect.height,
  };
  const videoComposited = samplePngRegion(
    postClickPng,
    videoScreenRect,
    (r, g, b) => Math.abs(r - 23) > 8 || Math.abs(g - 38) > 8 || Math.abs(b - 56) > 8,
  );
  assert.ok(as2ShotF6.matched > 0, `${id}: AS2 nested clip pixel must change to magenta on frame 6`);
  assert.ok(as3ShotF6.matched > 0, `${id}: AS3 nested clip pixel must change to gold on frame 6`);
  assert.ok(inputInkPixels.matched >= 40, `${id}: Edited input text must render dark glyph ink on Stage`);
  assert.ok(videoComposited.matched >= 200, `${id}: Gecko compositor must display decoded video pixels`);

  // 6. Pointer hover + click on interactive MovieClip (`netMc` at stage 230,240 size 180x52 -> center 320,266)
  //    Triggers AS3 URLLoader("sibling-payload.json") + AS2 LoadVars("sibling-vars.txt")!
  const mcStageX = 320;
  const mcStageY = 266;
  const mcClient = stageToClient(mcStageX, mcStageY);
  await bidi.hoverClientPoint(mcClient.x, mcClient.y);
  await bidi.clickClientPoint(mcClient.x, mcClient.y);

  const afterNet = await bidi.waitFor(`${id} MovieClip click + URL loading`, async () => {
    const g = await bidi.evalJson(`() => JSON.stringify(window.NeoFlashPlayer.root.getGateSnapshot())`);
    if (
      g.mcClicks < 1 ||
      g.as2McClicks < 1 ||
      g.siblingUrlStatus === 'idle' ||
      g.siblingUrlStatus === 'loading' ||
      g.as2SiblingVars === 'idle'
    ) {
      return null;
    }
    return g;
  });

  const [mx, my] = afterNet.mcClickCoords.split(',').map(Number);
  assert.ok(
    Math.abs(mx - mcStageX) <= 3 && Math.abs(my - mcStageY) <= 3,
    `${id}: MovieClip stageX/stageY expected ~${mcStageX},${mcStageY}, got ${afterNet.mcClickCoords}`,
  );
  assert.ok(afterNet.mcRollOvers >= 1, `${id}: MovieClip AS3 ROLL_OVER must fire`);
  assert.ok(afterNet.as2HoverCount >= 1, `${id}: MovieClip AS2 onRollOver must fire`);

  if (protocol === 'http') {
    assert.equal(
      afterNet.siblingUrlStatus,
      'ok:{"gate":"firefox-http-ok","items":3}',
      `${id}: HTTP URLLoader must load sibling JSON payload`,
    );
    assert.equal(
      afterNet.as2SiblingVars,
      'ok:http_ready:gecko',
      `${id}: HTTP AS2 LoadVars must parse sibling variables`,
    );

    // Also verify HTTP progressive/Range video playback (`sibling-video.mp4`) in Shared HTTP mode!
    await bidi.evalJson(`() => {
      window.NeoFlashPlayer.root.videoMetaCount = 0;
      window.NeoFlashPlayer.root.playVideoUrl("sibling-video.mp4");
      return JSON.stringify(true);
    }`);
    const httpVid = await bidi.waitFor(`${id} HTTP sibling video decode`, async () => {
      const g = await bidi.evalJson(`() => JSON.stringify(window.NeoFlashPlayer.root.getGateSnapshot())`);
      return g.videoSourcePlayed === 'sibling-video.mp4' && g.videoMetaCount >= 1 ? g : null;
    });
    assert.ok(httpVid.videoDuration > 0, `${id}: HTTP sibling MP4 must decode metadata and frames`);
  } else {
    assert.equal(
      afterNet.siblingUrlStatus,
      'io_error_handled',
      `${id}: file:// unbundled URLLoader must fail gracefully via IOErrorEvent without crashing`,
    );
    assert.equal(
      afterNet.as2SiblingVars,
      'io_error_handled',
      `${id}: file:// unbundled LoadVars must call onLoad(false) cleanly without crashing`,
    );
  }

  // 7. Player `Restart`: click real `#restart` HUD button and assert clean reset + zero duplicate listeners
  const restartBtnPt = await bidi.evalJson(`() => {
    const btn = document.getElementById('restart');
    const r = btn.getBoundingClientRect();
    return JSON.stringify({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  }`);
  await bidi.clickClientPoint(restartBtnPt.x, restartBtnPt.y);

  const afterRestart = await bidi.waitFor(`${id} Restart reset`, async () => {
    const snap = await bidi.evalJson(`() => {
      const g = window.NeoFlashPlayer.root.getGateSnapshot();
      if (
        g.currentFrame < 4 ||
        g.as2NestedPhase !== 'boot_f3' ||
        g.spinnerPhase !== 'boot_f3' ||
        g.inputText !== 'ABCD'
      ) {
        return JSON.stringify(null);
      }
      const domRoot = document.getElementById('stage-dom-root');
      return JSON.stringify({
        g,
        domMountCount: domRoot ? domRoot.mountCount : null,
        domDisposeCount: domRoot ? domRoot.disposeCount : null,
        domInputCount: document.querySelectorAll('.nf-dom-input').length,
      });
    }`);
    return snap;
  });

  assert.equal(afterRestart.g.btnClicks, 0, `${id}: Restart must reset AS3 click state`);
  assert.equal(afterRestart.g.as2BtnClicks || 0, 0, `${id}: Restart must reset AS2 click state`);
  assert.equal(afterRestart.g.inputText, 'ABCD', `${id}: Restart must reset input TextField to ABCD`);
  assert.equal(afterRestart.g.videoMetaCount, 0, `${id}: Restart must reset NetStream/Video session`);
  if (renderMode === 'dom') {
    assert.equal(afterRestart.domMountCount, 1, `${id}: DOM stage must stay mounted once across Restart`);
    assert.equal(afterRestart.domDisposeCount, 0, `${id}: Restart must not dispose <neoflash-stage>`);
    assert.equal(afterRestart.domInputCount, 1, `${id}: Restart must not leak duplicate DOM inputs`);
  }

  // Click actionBtn once after Restart to prove listeners fire once (not duplicated!)
  await bidi.clickClientPoint(btnClient.x, btnClient.y);
  const postRestartClick = await bidi.waitFor(`${id} post-Restart single listener check`, async () => {
    const g = await bidi.evalJson(`() => JSON.stringify(window.NeoFlashPlayer.root.getGateSnapshot())`);
    return g.btnClicks >= 1 ? g : null;
  });
  assert.equal(postRestartClick.btnClicks, 1, `${id}: post-Restart click must fire listener exactly once`);
  assert.equal(postRestartClick.as2BtnClicks, 1, `${id}: post-Restart AS2 click must fire exactly once`);

  const preUnloadErrors = await bidi.collectHardErrors();
  assertZeroErrors(id, preUnloadErrors);

  // 8. Unload (`pagehide` / `beforeunload`) teardown check
  await verifyUnloadLifecycle(bidi, id, renderMode, blankUrl);

  return {
    id,
    sizeLevel,
    renderMode,
    protocol,
    passed: true,
  };
}

async function runCompactBannerScenario(bidi, spec) {
  const { id, url, blankUrl, renderMode } = spec;
  bidi.resetScenarioLogs();

  await bidi.send('browsingContext.navigate', {
    context: bidi.context,
    url,
    wait: 'complete',
  });
  await bidi.installContentMonitor();

  const boot = await bidi.waitFor(`${id} compact scriptless banner boot`, async () => {
    const snap = await bidi.evalJson(`() => {
      const errEl = document.getElementById('error');
      if (errEl && errEl.style.display === 'block') {
        return JSON.stringify({ bootError: errEl.textContent });
      }
      const status = document.getElementById('status')?.textContent || '';
      const m = status.match(/^(\\d+)\\s*\\/\\s*12/);
      if (!m || Number(m[1]) < 2 || !window.NeoFlashPlayer) return JSON.stringify(null);
      const surfaceEl = ${renderMode === 'dom' ? "document.getElementById('stage-dom-root')" : "document.getElementById('stage')"};
      const r = surfaceEl.getBoundingClientRect();
      return JSON.stringify({
        origin: window.origin,
        frame: Number(m[1]),
        fontLoaded: document.fonts.check('32px "${EMBEDDED_FONT_FAMILY}"'),
        rect: { left: r.left, top: r.top, width: r.width, height: r.height },
      });
    }`);
    if (snap?.bootError) throw new Error(`${id} boot error:\n${snap.bootError}`);
    return snap;
  });

  assert.equal(boot.origin, 'null', `${id}: file:// strict origin expected`);
  assert.equal(boot.fontLoaded, true, `${id}: embedded font must load in scriptless Compact banner`);

  // Unlock audio via pointer click on stage and verify screenshot has non-blank tween + glyph pixels
  await bidi.clickClientPoint(boot.rect.left + 80, boot.rect.top + 80);
  await bidi.waitFor(`${id} banner audio unlock`, async () => {
    const txt = await bidi.evalJson(`() => JSON.stringify(document.getElementById('audio-status')?.textContent || '')`);
    return txt && !txt.includes('waiting for a gesture') ? txt : null;
  });

  const png = await bidi.capturePng();
  const nonBg = samplePngRegion(
    png,
    { x: boot.rect.left + 10, y: boot.rect.top + 10, width: boot.rect.width - 20, height: boot.rect.height - 20 },
    (r, g, b) => Math.abs(r - 18) > 12 || Math.abs(g - 26) > 12 || Math.abs(b - 40) > 12,
  );
  assert.ok(nonBg.matched >= 400, `${id}: Compact banner must render non-blank Stage pixels (${nonBg.matched})`);

  // Restart + unload
  const restartPt = await bidi.evalJson(`() => {
    window.NeoFlashPlayer.pause();
    const r = document.getElementById('restart').getBoundingClientRect();
    return JSON.stringify({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  }`);
  await bidi.clickClientPoint(restartPt.x, restartPt.y);
  await bidi.clickClientPoint(boot.rect.left + 80, boot.rect.top + 80);
  await bidi.waitFor(`${id} post-Restart banner tick`, async () => {
    const snap = await bidi.evalJson(`() => {
      const status = document.getElementById('status')?.textContent || '';
      const audioToggle = document.getElementById('audio-toggle');
      const m = status.match(/^(\\d+)\\s*\\/\\s*12/);
      return JSON.stringify(
        m && Number(m[1]) >= 2 && audioToggle && !audioToggle.hidden ? status : null
      );
    }`);
    return snap;
  });

  const errs = await bidi.collectHardErrors();
  assertZeroErrors(id, errs);
  await verifyUnloadLifecycle(bidi, id, renderMode, blankUrl);

  return {
    id,
    sizeLevel: 'compact-scriptless',
    renderMode,
    protocol: 'file',
    passed: true,
  };
}

// ---------------------------------------------------------------------------
// Export helpers & package structure assertions
// ---------------------------------------------------------------------------
function runExport(args) {
  execFileSync(EXPORTER_BIN, args, {
    cwd: ROOT_DIR,
    env: augmentedEnv(),
    stdio: 'pipe',
  });
}

function assertExportedPackageStructure(dir, sizeLevel, isScriptless = false) {
  assert.ok(fs.existsSync(path.join(dir, 'index.html')), `${sizeLevel}: index.html must exist`);
  assert.ok(fs.existsSync(path.join(dir, 'bundle.js')), `${sizeLevel}: bundle.js must exist`);
  const bundleText = fs.readFileSync(path.join(dir, 'bundle.js'), 'utf8');

  if (sizeLevel === 'full') {
    assert.ok(bundleText.includes('"wasm":'), 'Full bundle.js must inline base64 WASM');
    assert.ok(!fs.existsSync(path.join(dir, 'neoflash_web_bg.wasm')), 'Full must not emit external .wasm');
  } else if (sizeLevel === 'balanced') {
    assert.ok(bundleText.includes('"wasm_url":"neoflash_web_bg.wasm"'), 'Balanced must reference local wasm_url');
    assert.ok(fs.existsSync(path.join(dir, 'neoflash_web_bg.wasm')), 'Balanced must emit neoflash_web_bg.wasm');
    assert.ok(fs.existsSync(path.join(dir, 'air.js')), 'Balanced must keep all optional families');
    const bindings = fs.readFileSync(path.join(dir, 'neoflash_web.js'), 'utf8');
    assert.ok(!bindings.includes('NeoFlashWasmBase64'), 'Balanced neoflash_web.js must not inline base64 WASM');
    const fallback = fs.readFileSync(path.join(dir, 'neoflash_wasm_fallback.js'), 'utf8');
    assert.ok(fallback.includes('globalThis.NeoFlashWasmBase64 ='), 'Balanced must include file:// WASM fallback');
  } else if (sizeLevel === 'compact') {
    assert.ok(fs.existsSync(path.join(dir, 'neoflash_web_bg.wasm')), 'Compact must emit neoflash_web_bg.wasm');
    assert.ok(!fs.existsSync(path.join(dir, 'air.js')), 'Compact must omit unused air.js family');
    assert.ok(!fs.existsSync(path.join(dir, 'as3-fte-3d.js')), 'Compact must omit unused as3-fte-3d.js family');
    if (isScriptless) {
      assert.ok(!fs.existsSync(path.join(dir, 'as2-object.js')), 'Scriptless Compact must omit SCRIPT_ONLY modules');
      assert.ok(!fs.existsSync(path.join(dir, 'timers.js')), 'Scriptless Compact must omit timers.js');
    }
    const bindings = fs.readFileSync(path.join(dir, 'neoflash_web.js'), 'utf8');
    assert.ok(!bindings.includes('NeoFlashWasmBase64'), 'Compact neoflash_web.js must not inline base64 WASM');
    const fallback = fs.readFileSync(path.join(dir, 'neoflash_wasm_fallback.js'), 'utf8');
    assert.ok(fallback.includes('globalThis.NeoFlashWasmBase64 ='), 'Compact must include file:// WASM fallback');
  } else if (sizeLevel === 'shared') {
    assert.ok(
      bundleText.includes('"wasm_url":"/shared-runtime/neoflash_web_bg.wasm"'),
      'Shared bundle.js must reference /shared-runtime/neoflash_web_bg.wasm',
    );
    assert.ok(!fs.existsSync(path.join(dir, 'neoflash_web_bg.wasm')), 'Shared bundle must not duplicate .wasm');
    assert.ok(!fs.existsSync(path.join(dir, 'runtime.js')), 'Shared bundle must not duplicate runtime.js');
  }
}

// ---------------------------------------------------------------------------
// Main gate entry point
// ---------------------------------------------------------------------------
export async function runFirefoxHtml5Gate(options = {}) {
  const quiet = Boolean(options.quiet);
  const outDir = options.outDir || DEFAULT_OUT_DIR;
  const filter = options.filter || '';

  const firefoxBin = resolveFirefoxBinary();
  if (!firefoxBin) {
    if (process.env.NEOFLASH_ALLOW_MISSING_FIREFOX === '1') {
      const skipMsg =
        '[SKIP] verify-firefox-html5-gate: Firefox binary not found and NEOFLASH_ALLOW_MISSING_FIREFOX=1 is set.';
      if (!quiet) console.log(skipMsg);
      return { skipped: true, reason: skipMsg, scenarios: [] };
    }
    throw new Error(
      'Firefox binary not found. Install Firefox (/Applications/Firefox.app on macOS or `firefox` on PATH), ' +
        'set NEOFLASH_GATE_FIREFOX=/path/to/firefox, or set NEOFLASH_ALLOW_MISSING_FIREFOX=1 on headless CI hosts without Firefox.',
    );
  }

  const versionOutput = execFileSync(firefoxBin, ['--version'], { encoding: 'utf8' }).trim();
  if (!quiet) console.log(`[firefox-gate] Using ${versionOutput} (${firefoxBin})`);

  ensureBuildArtifacts(quiet);

  fs.rmSync(outDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  const projectsDir = path.join(outDir, 'projects');
  const fileExportsDir = path.join(outDir, 'file-bundles');
  const httpRootDir = path.join(outDir, 'http-root');
  fs.mkdirSync(projectsDir, { recursive: true });
  fs.mkdirSync(fileExportsDir, { recursive: true });
  fs.mkdirSync(httpRootDir, { recursive: true });

  const assets = loadFixtureAssets();
  const suiteProjectPath = path.join(projectsDir, 'firefox-gate-suite.nflash');
  const bannerProjectPath = path.join(projectsDir, 'firefox-gate-banner.nflash');
  fs.writeFileSync(suiteProjectPath, JSON.stringify(buildSuiteProject(assets), null, 2), 'utf8');
  fs.writeFileSync(bannerProjectPath, JSON.stringify(buildScriptlessBannerProject(assets), null, 2), 'utf8');

  // Export Shared runtime once into <httpRootDir>/shared-runtime
  const sharedRuntimeDir = path.join(httpRootDir, 'shared-runtime');
  runExport(['--shared-runtime', sharedRuntimeDir, '--runtime', RUNTIME_DIR]);
  assert.ok(
    fs.existsSync(path.join(sharedRuntimeDir, 'neoflash_web_bg.wasm')),
    'Shared runtime folder must contain neoflash_web_bg.wasm',
  );

  // Export Full, Balanced, and Compact bundles for direct `file://` verification (Canvas + DOM)
  const fileLevels = ['full', 'balanced', 'compact'];
  const renderModes = ['canvas', 'dom'];

  for (const sizeLevel of fileLevels) {
    for (const renderMode of renderModes) {
      const bundleDir = path.join(fileExportsDir, `suite-${sizeLevel}-${renderMode}`);
      runExport([
        suiteProjectPath,
        '--runtime',
        RUNTIME_DIR,
        '--output',
        bundleDir,
        '--size-level',
        sizeLevel,
        '--render-mode',
        renderMode,
      ]);
      assertExportedPackageStructure(bundleDir, sizeLevel, false);
    }
  }

  for (const renderMode of renderModes) {
    const bannerDir = path.join(fileExportsDir, `banner-compact-${renderMode}`);
    runExport([
      bannerProjectPath,
      '--runtime',
      RUNTIME_DIR,
      '--output',
      bannerDir,
      '--size-level',
      'compact',
      '--render-mode',
      renderMode,
    ]);
    assertExportedPackageStructure(bannerDir, 'compact', true);
  }

  // Export Shared bundles for HTTP verification (Canvas + DOM) + write sibling HTTP payloads
  for (const renderMode of renderModes) {
    const sharedBundleDir = path.join(httpRootDir, `suite-shared-${renderMode}`);
    runExport([
      suiteProjectPath,
      '--runtime',
      RUNTIME_DIR,
      '--output',
      sharedBundleDir,
      '--size-level',
      'shared',
      '--runtime-url',
      '/shared-runtime',
      '--render-mode',
      renderMode,
    ]);
    assertExportedPackageStructure(sharedBundleDir, 'shared', false);
    fs.writeFileSync(
      path.join(sharedBundleDir, 'sibling-payload.json'),
      '{"gate":"firefox-http-ok","items":3}',
      'utf8',
    );
    fs.writeFileSync(
      path.join(sharedBundleDir, 'sibling-vars.txt'),
      'status=http_ready&engine=gecko',
      'utf8',
    );
    fs.writeFileSync(path.join(sharedBundleDir, 'sibling-video.mp4'), assets.motionMp4Bytes);
  }

  const blankHtml = '<!doctype html><meta charset="utf-8"><title>unloaded</title>\n';
  const fileBlankPath = path.join(fileExportsDir, 'blank.html');
  fs.writeFileSync(fileBlankPath, blankHtml, 'utf8');
  fs.writeFileSync(path.join(httpRootDir, 'blank.html'), blankHtml, 'utf8');
  const fileBlankUrl = pathToFileURL(fileBlankPath).href;

  const httpServer = await startStaticHttpServer(httpRootDir);
  const httpBlankUrl = `${httpServer.baseUrl}/blank.html`;
  let bidi = null;
  const results = [];

  try {
    bidi = await launchFirefoxBidi(firefoxBin);

    const suiteScenarios = [];
    for (const sizeLevel of fileLevels) {
      for (const renderMode of renderModes) {
        const indexPath = path.join(fileExportsDir, `suite-${sizeLevel}-${renderMode}`, 'index.html');
        suiteScenarios.push({
          id: `file-${sizeLevel}-${renderMode}`,
          url: pathToFileURL(indexPath).href,
          blankUrl: fileBlankUrl,
          protocol: 'file',
          sizeLevel,
          renderMode,
        });
      }
    }
    for (const renderMode of renderModes) {
      suiteScenarios.push({
        id: `http-shared-${renderMode}`,
        url: `${httpServer.baseUrl}/suite-shared-${renderMode}/index.html`,
        blankUrl: httpBlankUrl,
        protocol: 'http',
        sizeLevel: 'shared',
        renderMode,
      });
    }

    const bannerScenarios = renderModes.map(renderMode => ({
      id: `file-compact-scriptless-${renderMode}`,
      url: pathToFileURL(path.join(fileExportsDir, `banner-compact-${renderMode}`, 'index.html')).href,
      blankUrl: fileBlankUrl,
      renderMode,
    }));

    for (const spec of suiteScenarios) {
      if (filter && !spec.id.includes(filter)) continue;
      const t0 = Date.now();
      const res = await runSuiteScenario(bidi, spec);
      res.durationMs = Date.now() - t0;
      results.push(res);
      if (!quiet) console.log(`  [PASS] ${spec.id} (${res.durationMs} ms)`);
    }

    for (const spec of bannerScenarios) {
      if (filter && !spec.id.includes(filter)) continue;
      const t0 = Date.now();
      const res = await runCompactBannerScenario(bidi, spec);
      res.durationMs = Date.now() - t0;
      results.push(res);
      if (!quiet) console.log(`  [PASS] ${spec.id} (${res.durationMs} ms)`);
    }

    // Confirm Shared HTTP requests genuinely fetched /shared-runtime/neoflash_web_bg.wasm
    if (!filter || 'http-shared-canvas'.includes(filter) || 'http-shared-dom'.includes(filter)) {
      const wasmFetched = httpServer.requests.some(
        r => r.path === '/shared-runtime/neoflash_web_bg.wasm',
      );
      assert.equal(
        wasmFetched,
        true,
        'Shared HTTP scenarios must fetch external /shared-runtime/neoflash_web_bg.wasm',
      );
    }

    const report = {
      skipped: false,
      firefoxVersion: versionOutput,
      firefoxBinary: firefoxBin,
      scenarioCount: results.length,
      scenarios: results,
    };
    fs.writeFileSync(path.join(outDir, 'report.json'), JSON.stringify(report, null, 2), 'utf8');
    if (!quiet) {
      console.log(
        `[firefox-gate] All ${results.length} Firefox HTML5 scenarios passed (${versionOutput}).`,
      );
    }
    return report;
  } finally {
    if (bidi) await bidi.close();
    await httpServer.close();
  }
}

async function cliMain() {
  const args = process.argv.slice(2);
  let quiet = false;
  let filter = '';
  for (const arg of args) {
    if (arg === '--quiet' || arg === '-q') quiet = true;
    else if (arg.startsWith('--filter=')) filter = arg.slice('--filter='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/verify-firefox-html5-gate.mjs [--quiet] [--filter=<substring>]');
      return;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  await runFirefoxHtml5Gate({ quiet, filter });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  cliMain().catch(err => {
    console.error('[firefox-gate] FAILED:', err?.stack || err);
    process.exit(1);
  });
}
