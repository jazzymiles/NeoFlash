// End-to-end verification of AS3 Area 8 Events, Media, UI & Accessibility runtime classes
// (web/as3-media-events.js, web/events.js, web/net.js, web/video.js, web/display.js, web/stage.js, web/loader.js)
// backed by the shared Rust As3MediaEventsKernel & as_events schema in WASM.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const NeoFlashEvents = require('../events.js');
const NeoFlashByteArray = require('../byte-array.js');
const NeoFlashGeom = require('../geom.js');
const NeoFlashGeom3D = require('../geom3d.js');
const NeoFlashBitmap = require('../bitmap.js');
const NeoFlashAudio = require('../audio.js');
const NeoFlashStage = require('../stage.js');
const NeoFlashNet = require('../net.js');
const NeoFlashAIR = require('../air.js');
const NeoFlashAS3MediaEvents = require('../as3-media-events.js');

const runtimeDir = path.resolve(__dirname, '../../target/web-runtime');
const wasmContext = vm.createContext({
  TextEncoder,
  TextDecoder,
  WebAssembly,
  console,
  URL,
  Request,
  Response,
});
vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), wasmContext);
const wasm = vm.runInContext('NeoFlashWasm', wasmContext);
wasmContext.wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
const ready = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', wasmContext);

function createMediaEventsHarness(customProject = {}) {
  let renderCount = 0;
  let accUpdateCount = 0;
  let accEvents = [];
  let accActive = Boolean(customProject.accessibilityActive);

  let geom2d;
  const geom3d = NeoFlashGeom3D.create(wasm, { getPoint: () => geom2d.Point });
  geom2d = NeoFlashGeom.create(wasm, { getVector3D: () => geom3d.Vector3D });
  const binary = NeoFlashByteArray.create({ wasm });

  const dummyProject = {
    format_version: 3,
    name: customProject.name || 'AS3 Media Events Suite',
    width: customProject.width || 800,
    height: customProject.height || 600,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: customProject.resources || [],
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'l1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, elements: [], label: '', tween: null }],
            },
          ],
          scripts: [],
        },
      },
    ],
  };

  const player = new wasm.WebPlayer(JSON.stringify(dummyProject));
  const eventFamilies = NeoFlashEvents.createFamilies(wasm, {
    isByteArray: v => binary.ByteArray.__nfIs(v),
    requestRender: () => {
      renderCount++;
    },
  });
  const allEvents = { ...NeoFlashEvents, ...eventFamilies };
  const net = NeoFlashNet.create({
    events: allEvents,
    binary,
    resources: dummyProject.resources,
  });
  const bitmap = NeoFlashBitmap.create({
    player,
    geometry: geom2d,
    getBinary: () => binary,
    getFilters: () => null,
    displayRef: () => null,
    paintScene: () => {},
  });
  const audio = NeoFlashAudio.create({
    player,
    wasm,
    events: NeoFlashEvents,
    net,
    binary,
    resources: dummyProject.resources,
  });

  const stageHub = NeoFlashEvents.create({ alive: () => true, parentOf: () => null });
  const stageHost = NeoFlashStage.create({
    events: stageHub,
    width: dummyProject.width,
    height: dummyProject.height,
    frameRate: 24,
  });

  const air = NeoFlashAIR.create({
    wasm,
    events: allEvents,
    eventFamilies,
    binary,
    geometry: geom2d,
    net,
    getStage: () => stageHost.stage,
    getViewportSize: () => [dummyProject.width, dummyProject.height],
    project: dummyProject,
  });

  class MockLoaderInfo extends NeoFlashEvents.EventDispatcher {}

  const mediaEvents = NeoFlashAS3MediaEvents.create({
    wasm,
    events: allEvents,
    binary,
    geometry: geom2d,
    net,
    getAudio: () => audio,
    getStage: () => stageHost.stage,
    StageClass: NeoFlashStage.Stage,
    LoaderInfoClass: MockLoaderInfo,
    airBuiltins: air.builtins,
    getAccessibilityActive: () => accActive,
    onAccessibilityUpdate: () => {
      accUpdateCount++;
    },
    onAccessibilityEvent: (source, childID, eventType, nonHTML) => {
      accEvents.push({ source, childID, eventType, nonHTML });
    },
  });

  return {
    player,
    wasm,
    binary,
    geom: geom2d,
    bitmap,
    audio,
    net,
    stage: stageHost.stage,
    stageHost,
    air,
    mediaEvents,
    B: mediaEvents.builtins,
    E: allEvents,
    MockLoaderInfo,
    getRenderCount: () => renderCount,
    getAccUpdateCount: () => accUpdateCount,
    getAccEvents: () => accEvents,
    setAccActive: flag => {
      accActive = Boolean(flag);
    },
    dispose() {
      mediaEvents.dispose();
      air.dispose();
      stageHost.dispose();
      audio.dispose();
      bitmap.dispose();
      binary.dispose();
      player.free();
    },
  };
}

test('schema-driven flash.events families, gesture/touch extensions, and network/status events satisfy AS3 contracts', async () => {
  await ready;
  const h = createMediaEventsHarness();
  try {
    const { E, net, binary } = h;

    // GesturePhase, SoftKeyboardTrigger, ThrottleType constants
    assert.equal(E.GesturePhase.ALL, 'all');
    assert.equal(E.GesturePhase.BEGIN, 'begin');
    assert.equal(E.GesturePhase.UPDATE, 'update');
    assert.equal(E.GesturePhase.END, 'end');
    assert.equal(E.SoftKeyboardTrigger.CONTENT_TRIGGERED, 'contentTriggered');
    assert.equal(E.SoftKeyboardTrigger.USER_TRIGGERED, 'userTriggered');
    assert.equal(E.ThrottleType.PAUSE, 'pause');
    assert.equal(E.ThrottleType.RESUME, 'resume');
    assert.equal(E.ThrottleType.THROTTLE, 'throttle');

    // AccelerometerEvent
    const accEv = new E.AccelerometerEvent(E.AccelerometerEvent.UPDATE, false, false, 125.5, 0.1, -0.2, 9.8);
    assert.equal(accEv.timestamp, 125.5);
    assert.equal(accEv.accelerationX, 0.1);
    assert.equal(accEv.accelerationY, -0.2);
    assert.equal(accEv.accelerationZ, 9.8);
    assert.equal(accEv.clone().accelerationZ, 9.8);
    assert.match(accEv.toString(), /^\[AccelerometerEvent type="update"/);

    // ActivityEvent
    const actEv = new E.ActivityEvent(E.ActivityEvent.ACTIVITY, false, false, true);
    assert.equal(actEv.activating, true);
    assert.equal(actEv.clone().activating, true);
    assert.match(actEv.toString(), /^\[ActivityEvent type="activity".*activating=true\]$/);

    // AsyncErrorEvent
    const innerErr = new Error('boom');
    const asyncErr = new E.AsyncErrorEvent(E.AsyncErrorEvent.ASYNC_ERROR, false, false, 'async fail', innerErr);
    assert.equal(asyncErr.text, 'async fail');
    assert.equal(asyncErr.error, innerErr);
    assert.equal(asyncErr.clone().error, innerErr);
    assert.match(asyncErr.toString(), /^\[AsyncErrorEvent /);

    // ContextMenuEvent (including 6th param isMouseTargetInaccessible)
    const ownerObj = { id: 'owner' };
    const mouseObj = { id: 'target' };
    const cmEv = new E.ContextMenuEvent(
      E.ContextMenuEvent.MENU_SELECT,
      false,
      false,
      mouseObj,
      ownerObj,
      true
    );
    assert.equal(E.ContextMenuEvent.MENU_ITEM_SELECT, 'menuItemSelect');
    assert.equal(cmEv.mouseTarget, mouseObj);
    assert.equal(cmEv.contextMenuOwner, ownerObj);
    assert.equal(cmEv.isMouseTargetInaccessible, true);
    const cmClone = cmEv.clone();
    assert.equal(cmClone.isMouseTargetInaccessible, true);
    assert.match(cmEv.toString(), /isMouseTargetInaccessible=true/);

    // DataEvent
    const dataEv = new E.DataEvent(E.DataEvent.UPLOAD_COMPLETE_DATA, true, false, 'uploaded-ok');
    assert.equal(E.DataEvent.DATA, 'data');
    assert.equal(dataEv.data, 'uploaded-ok');
    assert.equal(dataEv.clone().data, 'uploaded-ok');
    assert.match(dataEv.toString(), /^\[DataEvent /);

    // GestureEvent, TransformGestureEvent, PressAndTapGestureEvent
    const prevRenders = h.getRenderCount();
    const gEv = new E.GestureEvent(
      E.GestureEvent.GESTURE_TWO_FINGER_TAP,
      true,
      false,
      E.GesturePhase.END,
      12,
      24,
      true,
      false,
      true,
      true,
      false
    );
    assert.equal(gEv.phase, 'end');
    assert.equal(gEv.localX, 12);
    assert.equal(gEv.localY, 24);
    assert.equal(gEv.ctrlKey, true);
    assert.equal(gEv.shiftKey, true);
    assert.equal(gEv.commandKey, true);
    assert.equal(gEv.controlKey, false);
    gEv.updateAfterEvent();
    assert.equal(h.getRenderCount(), prevRenders + 1);
    assert.equal(gEv.clone().localX, 12);
    assert.match(gEv.toString(), /^\[GestureEvent /);

    const tgEv = new E.TransformGestureEvent(
      E.TransformGestureEvent.GESTURE_ZOOM,
      true,
      false,
      E.GesturePhase.UPDATE,
      10,
      20,
      1.5,
      1.25,
      45,
      3,
      -4,
      true,
      false,
      true,
      true,
      true,
      2.75
    );
    assert.equal(E.TransformGestureEvent.GESTURE_PAN, 'gesturePan');
    assert.equal(E.TransformGestureEvent.GESTURE_ROTATE, 'gestureRotate');
    assert.equal(E.TransformGestureEvent.GESTURE_SWIPE, 'gestureSwipe');
    assert.equal(E.TransformGestureEvent.GESTURE_DIRECTIONAL_TAP, 'gestureDirectionalTap');
    assert.equal(tgEv.scaleX, 1.5);
    assert.equal(tgEv.scaleY, 1.25);
    assert.equal(tgEv.rotation, 45);
    assert.equal(tgEv.offsetX, 3);
    assert.equal(tgEv.offsetY, -4);
    assert.equal(tgEv.commandKey, true);
    assert.equal(tgEv.controlKey, true);
    assert.equal(tgEv.velocity, 2.75);
    assert.equal(tgEv.clone().velocity, 2.75);
    assert.match(tgEv.toString(), /^\[TransformGestureEvent /);

    const ptEv = new E.PressAndTapGestureEvent(
      E.PressAndTapGestureEvent.GESTURE_PRESS_AND_TAP,
      true,
      false,
      E.GesturePhase.ALL,
      5,
      6,
      15,
      16,
      false,
      true,
      false,
      true,
      false
    );
    assert.equal(ptEv.tapLocalX, 15);
    assert.equal(ptEv.tapLocalY, 16);
    assert.equal(ptEv.commandKey, true);
    assert.equal(ptEv.clone().tapLocalX, 15);
    assert.match(ptEv.toString(), /^\[PressAndTapGestureEvent /);

    // TouchEvent (including commandKey, controlKey, timestamp, touchIntent, samples, getSamples, isToolButtonDown, updateAfterEvent)
    const sampleBuf = new binary.ByteArray();
    sampleBuf.writeFloat(10);
    sampleBuf.writeFloat(20);
    sampleBuf.writeFloat(0.75);
    const touchEv = new E.TouchEvent(
      E.TouchEvent.TOUCH_BEGIN,
      true,
      false,
      7,
      true,
      30,
      40,
      8,
      9,
      0.75,
      mouseObj,
      true,
      false,
      true,
      true,
      false,
      999.5,
      'pen',
      sampleBuf,
      false
    );
    assert.equal(E.TouchEvent.TOUCH_END, 'touchEnd');
    assert.equal(E.TouchEvent.TOUCH_MOVE, 'touchMove');
    assert.equal(E.TouchEvent.TOUCH_OUT, 'touchOut');
    assert.equal(E.TouchEvent.TOUCH_OVER, 'touchOver');
    assert.equal(E.TouchEvent.TOUCH_ROLL_OUT, 'touchRollOut');
    assert.equal(E.TouchEvent.TOUCH_ROLL_OVER, 'touchRollOver');
    assert.equal(E.TouchEvent.TOUCH_TAP, 'touchTap');
    assert.equal(touchEv.touchPointID, 7);
    assert.equal(touchEv.isPrimaryTouchPoint, true);
    assert.equal(touchEv.localX, 30);
    assert.equal(touchEv.localY, 40);
    assert.equal(touchEv.sizeX, 8);
    assert.equal(touchEv.sizeY, 9);
    assert.equal(touchEv.pressure, 0.75);
    assert.equal(touchEv.relatedObject, mouseObj);
    assert.equal(touchEv.commandKey, true);
    assert.equal(touchEv.controlKey, false);
    assert.equal(touchEv.isToolButtonDown(0), false);
    const outSamples = new binary.ByteArray();
    assert.equal(touchEv.getSamples(outSamples, false), 1);
    assert.equal(outSamples.length, 12);
    touchEv.updateAfterEvent();
    assert.equal(h.getRenderCount(), prevRenders + 2);
    assert.equal(touchEv.clone().commandKey, true);
    assert.match(touchEv.toString(), /commandKey=true/);

    // IMEEvent, NetDataEvent, SampleDataEvent, SecurityErrorEvent, SoftKeyboardEvent, StatusEvent, SyncEvent, ThrottleEvent, UncaughtErrorEvent
    const imeClient = { confirmComposition() {} };
    const imeEv = new E.IMEEvent(E.IMEEvent.IME_START_COMPOSITION, false, false, 'kanji', imeClient);
    assert.equal(E.IMEEvent.IME_COMPOSITION, 'imeComposition');
    assert.equal(imeEv.imeClient, imeClient);
    assert.equal(imeEv.clone().text, 'kanji');
    assert.match(imeEv.toString(), /^\[IMEEvent /);

    const ndEv = new E.NetDataEvent(E.NetDataEvent.MEDIA_TYPE_DATA, false, false, 42.25, { handler: 'onMetaData' });
    assert.equal(ndEv.timestamp, 42.25);
    assert.deepEqual(ndEv.info, { handler: 'onMetaData' });
    assert.equal(ndEv.clone().timestamp, 42.25);
    assert.match(ndEv.toString(), /^\[NetDataEvent /);

    const pcm = new binary.ByteArray();
    pcm.writeFloat(0.25);
    const sdEv = new E.SampleDataEvent(E.SampleDataEvent.SAMPLE_DATA, false, false, 2048, pcm);
    assert.equal(sdEv.position, 2048);
    assert.equal(sdEv.data, pcm);
    assert.equal(sdEv.clone().position, 2048);
    assert.match(sdEv.toString(), /^\[SampleDataEvent /);

    const secEv = new E.SecurityErrorEvent(E.SecurityErrorEvent.SECURITY_ERROR, false, false, 'sandbox denied', 2048);
    assert.equal(secEv.text, 'sandbox denied');
    assert.equal(secEv.errorID, 2048);
    assert.equal(secEv.clone().errorID, 2048);
    assert.match(secEv.toString(), /^\[SecurityErrorEvent /);

    const skEv = new E.SoftKeyboardEvent(
      E.SoftKeyboardEvent.SOFT_KEYBOARD_ACTIVATE,
      true,
      true,
      mouseObj,
      E.SoftKeyboardTrigger.USER_TRIGGERED
    );
    assert.equal(E.SoftKeyboardEvent.SOFT_KEYBOARD_ACTIVATING, 'softKeyboardActivating');
    assert.equal(E.SoftKeyboardEvent.SOFT_KEYBOARD_DEACTIVATE, 'softKeyboardDeactivate');
    assert.equal(skEv.relatedObject, mouseObj);
    assert.equal(skEv.triggerType, 'userTriggered');
    assert.equal(skEv.clone().triggerType, 'userTriggered');
    assert.match(skEv.toString(), /^\[SoftKeyboardEvent /);

    const stEv = new E.StatusEvent(E.StatusEvent.STATUS, false, false, 'Camera.Unmuted', 'status');
    assert.equal(stEv.code, 'Camera.Unmuted');
    assert.equal(stEv.level, 'status');
    assert.equal(stEv.clone().code, 'Camera.Unmuted');
    assert.match(stEv.toString(), /^\[StatusEvent /);

    const syncList = [{ code: 'clear' }];
    const syEv = new E.SyncEvent(E.SyncEvent.SYNC, false, false, syncList);
    assert.equal(syEv.changeList, syncList);
    assert.equal(syEv.clone().changeList, syncList);
    assert.match(syEv.toString(), /^\[SyncEvent /);

    const thEv = new E.ThrottleEvent(E.ThrottleEvent.THROTTLE, false, false, E.ThrottleType.THROTTLE, 4);
    assert.equal(thEv.state, 'throttle');
    assert.equal(thEv.targetFrameRate, 4);
    assert.equal(thEv.clone().targetFrameRate, 4);
    assert.match(thEv.toString(), /^\[ThrottleEvent /);

    const ueEv = new E.UncaughtErrorEvent(E.UncaughtErrorEvent.UNCAUGHT_ERROR, true, true, innerErr);
    assert.equal(ueEv.error, innerErr);
    assert.equal(ueEv.clone().error, innerErr);
    assert.match(ueEv.toString(), /^\[UncaughtErrorEvent /);

    // ProgressEvent, IOErrorEvent, HTTPStatusEvent
    assert.equal(net.ProgressEvent.PROGRESS, 'progress');
    assert.equal(net.ProgressEvent.SOCKET_DATA, 'socketData');
    const progEv = new net.ProgressEvent(net.ProgressEvent.SOCKET_DATA, false, false, 64, 128);
    assert.equal(progEv.bytesLoaded, 64);
    assert.equal(progEv.bytesTotal, 128);
    assert.equal(progEv.clone().bytesTotal, 128);
    assert.match(progEv.toString(), /^\[ProgressEvent type="socketData".*bytesLoaded=64 bytesTotal=128\]$/);

    assert.equal(net.IOErrorEvent.IO_ERROR, 'ioError');
    const ioEv = new net.IOErrorEvent(net.IOErrorEvent.IO_ERROR, false, false, '404 missing', 2032);
    assert.equal(ioEv.text, '404 missing');
    assert.equal(ioEv.clone().text, '404 missing');
    assert.match(ioEv.toString(), /^\[IOErrorEvent type="ioError"/);

    assert.equal(net.HTTPStatusEvent.HTTP_STATUS, 'httpStatus');
    assert.equal(net.HTTPStatusEvent.HTTP_RESPONSE_STATUS, 'httpResponseStatus');
    const httpEv = new net.HTTPStatusEvent(net.HTTPStatusEvent.HTTP_STATUS, false, false, 302, true);
    assert.equal(httpEv.status, 302);
    assert.equal(httpEv.redirected, true);
    httpEv.redirected = false;
    assert.equal(httpEv.redirected, false);
    assert.equal(httpEv.clone().status, 302);
    assert.match(httpEv.toString(), /^\[HTTPStatusEvent type="httpStatus"/);
  } finally {
    h.dispose();
  }
});

test('all 21 new flash.events.* classes in web/as3-media-events.js implement Adobe constructors, properties, clone, toString, and LoaderInfo.uncaughtErrorEvents', async () => {
  await ready;
  const h = createMediaEventsHarness();
  try {
    const { B, binary, bitmap, MockLoaderInfo } = h;

    // 1. AVDictionaryDataEvent
    const AVDictionaryDataEvent = B['flash.events.AVDictionaryDataEvent'];
    const dictObj = { key: 'val' };
    const avDict = new AVDictionaryDataEvent(
      AVDictionaryDataEvent.AV_DICTIONARY_DATA,
      false,
      false,
      dictObj,
      12.5
    );
    assert.equal(avDict.dictionary, dictObj);
    assert.equal(avDict.time, 12.5);
    assert.equal(avDict.clone().dictionary, dictObj);
    assert.match(avDict.toString(), /^\[AVDictionaryDataEvent /);

    // 2. AVHTTPStatusEvent
    const AVHTTPStatusEvent = B['flash.events.AVHTTPStatusEvent'];
    const avHttp = new AVHTTPStatusEvent(
      AVHTTPStatusEvent.AV_HTTP_RESPONSE_STATUS,
      false,
      false,
      206,
      'https://cdn.example/seg.ts',
      [{ name: 'Accept-Ranges', value: 'bytes' }]
    );
    assert.equal(avHttp.status, 206);
    assert.equal(avHttp.responseURL, 'https://cdn.example/seg.ts');
    assert.equal(avHttp.responseHeaders.length, 1);
    assert.equal(avHttp.clone().status, 206);
    assert.match(avHttp.toString(), /^\[AVHTTPStatusEvent .*status=206/);

    // 3. AudioOutputChangeEvent
    const AudioOutputChangeEvent = B['flash.events.AudioOutputChangeEvent'];
    const AudioOutputChangeReason = B['flash.media.AudioOutputChangeReason'];
    const aoEv = new AudioOutputChangeEvent(
      AudioOutputChangeEvent.AUDIO_OUTPUT_CHANGE,
      false,
      false,
      AudioOutputChangeReason.USER_SELECTION
    );
    assert.equal(aoEv.reason, 'userSelection');
    assert.equal(aoEv.clone().reason, 'userSelection');
    assert.match(aoEv.toString(), /^\[AudioOutputChangeEvent /);

    // 4-13. DRM* event classes
    const DRMAuthenticationCompleteEvent = B['flash.events.DRMAuthenticationCompleteEvent'];
    const tok = new binary.ByteArray();
    tok.writeUTFBytes('jwt-token');
    const drmAuthOk = new DRMAuthenticationCompleteEvent(
      DRMAuthenticationCompleteEvent.AUTHENTICATION_COMPLETE,
      false,
      false,
      'https://drm.example',
      'main',
      tok
    );
    assert.equal(drmAuthOk.serverURL, 'https://drm.example');
    assert.equal(drmAuthOk.domain, 'main');
    assert.equal(drmAuthOk.token, tok);
    drmAuthOk.domain = 'alt';
    assert.equal(drmAuthOk.domain, 'alt');
    assert.equal(drmAuthOk.clone().domain, 'alt');
    assert.match(drmAuthOk.toString(), /^\[DRMAuthenticationCompleteEvent /);

    const DRMAuthenticationErrorEvent = B['flash.events.DRMAuthenticationErrorEvent'];
    const drmAuthErr = new DRMAuthenticationErrorEvent(
      DRMAuthenticationErrorEvent.AUTHENTICATION_ERROR,
      false,
      false,
      'bad creds',
      3305,
      101,
      'https://drm.example',
      'main'
    );
    assert.equal(drmAuthErr.text, 'bad creds');
    assert.equal(drmAuthErr.errorID, 3305);
    assert.equal(drmAuthErr.subErrorID, 101);
    assert.equal(drmAuthErr.serverURL, 'https://drm.example');
    assert.equal(drmAuthErr.domain, 'main');
    assert.equal(drmAuthErr.clone().subErrorID, 101);
    assert.match(drmAuthErr.toString(), /^\[DRMAuthenticationErrorEvent /);

    const DRMDeviceGroupEvent = B['flash.events.DRMDeviceGroupEvent'];
    const grp = { domain: 'group-a' };
    const drmGrp = new DRMDeviceGroupEvent(
      DRMDeviceGroupEvent.ADD_TO_DEVICE_GROUP_COMPLETE,
      false,
      false,
      grp
    );
    assert.equal(drmGrp.deviceGroup, grp);
    assert.equal(drmGrp.clone().deviceGroup, grp);
    assert.match(drmGrp.toString(), /^\[DRMDeviceGroupEvent /);

    const DRMDeviceGroupErrorEvent = B['flash.events.DRMDeviceGroupErrorEvent'];
    const drmGrpErr = new DRMDeviceGroupErrorEvent(
      DRMDeviceGroupErrorEvent.ADD_TO_DEVICE_GROUP_ERROR,
      false,
      false,
      'group error',
      3320,
      7,
      grp,
      true,
      false
    );
    assert.equal(drmGrpErr.subErrorID, 7);
    assert.equal(drmGrpErr.deviceGroup, grp);
    assert.equal(drmGrpErr.systemUpdateNeeded, true);
    assert.equal(drmGrpErr.drmUpdateNeeded, false);
    assert.equal(drmGrpErr.clone().subErrorID, 7);
    assert.match(drmGrpErr.toString(), /^\[DRMDeviceGroupErrorEvent /);

    const DRMErrorEvent = B['flash.events.DRMErrorEvent'];
    const cData = { licenseID: 'lic-1' };
    const drmErr = new DRMErrorEvent(
      DRMErrorEvent.DRM_ERROR,
      false,
      false,
      'voucher invalid',
      3301,
      55,
      cData,
      false,
      true
    );
    assert.equal(drmErr.contentData, cData);
    assert.equal(drmErr.subErrorID, 55);
    assert.equal(drmErr.systemUpdateNeeded, false);
    assert.equal(drmErr.drmUpdateNeeded, true);
    assert.equal(drmErr.clone().drmUpdateNeeded, true);
    assert.match(drmErr.toString(), /^\[DRMErrorEvent /);

    const DRMLicenseRequestEvent = B['flash.events.DRMLicenseRequestEvent'];
    const drmLic = new DRMLicenseRequestEvent(
      DRMLicenseRequestEvent.LICENSE_REQUEST,
      false,
      false,
      'https://lic.example'
    );
    assert.equal(drmLic.serverURL, 'https://lic.example');
    drmLic.serverURL = 'https://lic2.example';
    assert.equal(drmLic.clone().serverURL, 'https://lic2.example');
    assert.match(drmLic.toString(), /^\[DRMLicenseRequestEvent /);

    const DRMMetadataEvent = B['flash.events.DRMMetadataEvent'];
    const drmMeta = new DRMMetadataEvent(DRMMetadataEvent.DRM_METADATA, false, false, cData, 88.25);
    assert.equal(drmMeta.drmMetadata, cData);
    assert.equal(drmMeta.timestamp, 88.25);
    assert.equal(drmMeta.clone().timestamp, 88.25);
    assert.match(drmMeta.toString(), /^\[DRMMetadataEvent /);

    const DRMReturnVoucherCompleteEvent = B['flash.events.DRMReturnVoucherCompleteEvent'];
    const drmRetOk = new DRMReturnVoucherCompleteEvent(
      DRMReturnVoucherCompleteEvent.RETURN_VOUCHER_COMPLETE,
      false,
      false,
      'https://drm.example',
      'lic-1',
      'pol-9',
      3
    );
    assert.equal(drmRetOk.serverURL, 'https://drm.example');
    assert.equal(drmRetOk.licenseID, 'lic-1');
    assert.equal(drmRetOk.policyID, 'pol-9');
    assert.equal(drmRetOk.numberOfVouchersReturned, 3);
    assert.equal(drmRetOk.clone().numberOfVouchersReturned, 3);
    assert.match(drmRetOk.toString(), /^\[DRMReturnVoucherCompleteEvent /);

    const DRMReturnVoucherErrorEvent = B['flash.events.DRMReturnVoucherErrorEvent'];
    const drmRetErr = new DRMReturnVoucherErrorEvent(
      DRMReturnVoucherErrorEvent.RETURN_VOUCHER_ERROR,
      false,
      false,
      'return failed',
      3315,
      19,
      'https://drm.example',
      'lic-1',
      'pol-9'
    );
    assert.equal(drmRetErr.subErrorID, 19);
    assert.equal(drmRetErr.licenseID, 'lic-1');
    assert.equal(drmRetErr.policyID, 'pol-9');
    assert.equal(drmRetErr.serverURL, 'https://drm.example');
    assert.equal(drmRetErr.clone().subErrorID, 19);
    assert.match(drmRetErr.toString(), /^\[DRMReturnVoucherErrorEvent /);

    const DRMStatusEvent = B['flash.events.DRMStatusEvent'];
    const voucherObj = { voucherEndDate: null };
    const drmStatus = new DRMStatusEvent(
      DRMStatusEvent.DRM_STATUS,
      false,
      false,
      cData,
      voucherObj,
      true
    );
    assert.equal(drmStatus.contentData, cData);
    assert.equal(drmStatus.voucher, voucherObj);
    assert.equal(drmStatus.isLocal, true);
    assert.equal(drmStatus.clone().isLocal, true);
    assert.match(drmStatus.toString(), /^\[DRMStatusEvent /);

    // 14-15. NetFilterEvent & NetMonitorEvent
    const NetFilterEvent = B['flash.events.NetFilterEvent'];
    const hdrBytes = new binary.ByteArray();
    const dataBytes = new binary.ByteArray();
    const nfEv = new NetFilterEvent('netFilter', false, false, hdrBytes, dataBytes);
    assert.equal(nfEv.header, hdrBytes);
    assert.equal(nfEv.data, dataBytes);
    assert.equal(nfEv.clone().header, hdrBytes);
    assert.match(nfEv.toString(), /^\[NetFilterEvent /);

    const NetMonitorEvent = B['flash.events.NetMonitorEvent'];
    const fakeStream = { time: 0 };
    const nmEv = new NetMonitorEvent(NetMonitorEvent.NET_STREAM_CREATE, false, false, fakeStream);
    assert.equal(nmEv.netStream, fakeStream);
    assert.equal(nmEv.clone().netStream, fakeStream);
    assert.match(nmEv.toString(), /^\[NetMonitorEvent /);

    // 16. ShaderEvent
    const ShaderEvent = B['flash.events.ShaderEvent'];
    const bmd = new bitmap.BitmapData(4, 4, true, 0xff112233);
    const shaderBa = new binary.ByteArray();
    const shaderVec = [0.5, 1.0];
    const shEv = new ShaderEvent(ShaderEvent.COMPLETE, false, false, bmd, shaderBa, shaderVec);
    assert.equal(shEv.bitmapData, bmd);
    assert.equal(shEv.byteArray, shaderBa);
    assert.equal(shEv.vector, shaderVec);
    assert.equal(shEv.clone().bitmapData, bmd);
    assert.match(shEv.toString(), /^\[ShaderEvent /);
    bmd.dispose();

    // 17-21. StageVideoAvailabilityEvent, StageVideoEvent, UncaughtErrorEvents, VideoEvent, VideoTextureEvent
    const StageVideoAvailabilityEvent = B['flash.events.StageVideoAvailabilityEvent'];
    const StageVideoAvailability = B['flash.media.StageVideoAvailability'];
    const StageVideoAvailabilityReason = B['flash.media.StageVideoAvailabilityReason'];
    const svaEv = new StageVideoAvailabilityEvent(
      StageVideoAvailabilityEvent.STAGE_VIDEO_AVAILABILITY,
      false,
      false,
      StageVideoAvailability.AVAILABLE,
      StageVideoAvailabilityReason.NO_ERROR
    );
    assert.equal(svaEv.availability, 'available');
    assert.equal(svaEv.reason, 'noError');
    assert.equal(svaEv.clone().availability, 'available');
    assert.match(svaEv.toString(), /^\[StageVideoAvailabilityEvent /);

    const StageVideoEvent = B['flash.events.StageVideoEvent'];
    const svEv = new StageVideoEvent(
      StageVideoEvent.RENDER_STATE,
      false,
      false,
      'accelerated',
      'BT.709'
    );
    assert.equal(svEv.status, 'accelerated');
    assert.equal(svEv.colorSpace, 'BT.709');
    assert.equal(svEv.clone().colorSpace, 'BT.709');
    assert.match(svEv.toString(), /^\[StageVideoEvent /);

    const VideoEvent = B['flash.events.VideoEvent'];
    assert.equal(VideoEvent.RENDER_STATE, 'renderState');
    assert.equal(VideoEvent.RENDER_STATUS_ACCELERATED, 'accelerated');
    assert.equal(VideoEvent.RENDER_STATUS_SOFTWARE, 'software');
    assert.equal(VideoEvent.RENDER_STATUS_UNAVAILABLE, 'unavailable');
    const vidEv = new VideoEvent(VideoEvent.RENDER_STATE, false, false, VideoEvent.RENDER_STATUS_SOFTWARE);
    assert.equal(vidEv.status, 'software');
    assert.equal(vidEv.clone().status, 'software');
    assert.match(vidEv.toString(), /^\[VideoEvent /);

    const VideoTextureEvent = B['flash.events.VideoTextureEvent'];
    const vtEv = new VideoTextureEvent(VideoTextureEvent.RENDER_STATE, false, false, 'accelerated', 'BT.601');
    assert.equal(vtEv.status, 'accelerated');
    assert.equal(vtEv.colorSpace, 'BT.601');
    assert.equal(vtEv.clone().colorSpace, 'BT.601');
    assert.match(vtEv.toString(), /^\[VideoTextureEvent /);

    const UncaughtErrorEvents = B['flash.events.UncaughtErrorEvents'];
    const uee = new UncaughtErrorEvents();
    let caught = null;
    uee.addEventListener('uncaughtError', e => {
      caught = e.error;
    });
    const boom = new Error('uncaught!');
    uee.dispatchEvent(new h.E.UncaughtErrorEvent('uncaughtError', true, true, boom));
    assert.equal(caught, boom);

    const li = new MockLoaderInfo();
    assert.ok(UncaughtErrorEvents.__nfIs(li.uncaughtErrorEvents));
    assert.equal(li.uncaughtErrorEvents, li.uncaughtErrorEvents);
  } finally {
    h.dispose();
  }
});

test('flash.media.* Camera, Microphone, StageVideo, VideoStreamSettings, AudioDeviceManager, AVURLLoader and AVURLStream execute via Rust kernel', async () => {
  await ready;
  const manifestPayload = Buffer.from('#EXTM3U\n#EXT-X-VERSION:3\n', 'utf8').toString('base64');
  const h = createMediaEventsHarness({
    resources: [
      {
        id: 'hls-manifest',
        path: 'media/stream.m3u8',
        media_type: 'application/vnd.apple.mpegurl',
        data_base64: manifestPayload,
      },
    ],
  });
  try {
    const { B, binary, geom, bitmap, stage } = h;

    // AVTagData, AVURLLoader, AVURLStream
    const AVTagData = B['flash.media.AVTagData'];
    const tag = new AVTagData('EXT-X-PROGRAM-DATE-TIME:2026-11-02', 14.25);
    assert.equal(tag.data, 'EXT-X-PROGRAM-DATE-TIME:2026-11-02');
    assert.equal(tag.localTime, 14.25);

    const AVURLLoader = B['flash.media.AVURLLoader'];
    const avLoader = new AVURLLoader();
    const loaderEvents = [];
    for (const type of ['open', 'httpStatus', 'progress', 'complete']) {
      avLoader.addEventListener(type, e => loaderEvents.push(e.type));
    }
    avLoader.cookieHeader = 'SID=123';
    assert.equal(avLoader.cookieHeader, 'SID=123');
    await new Promise((resolve, reject) => {
      avLoader.addEventListener('complete', resolve);
      avLoader.addEventListener('ioError', e => reject(new Error(e.text)));
      avLoader.load(new h.net.URLRequest('media/stream.m3u8'));
    });
    assert.ok(loaderEvents.includes('open'));
    assert.ok(loaderEvents.includes('progress'));
    assert.ok(loaderEvents.includes('complete'));
    assert.match(String(avLoader.data), /#EXTM3U/);
    avLoader.close();

    const AVURLStream = B['flash.media.AVURLStream'];
    const avStream = new AVURLStream();
    avStream.cookieHeader = 'SID=456';
    let streamDone = false;
    await new Promise((resolve, reject) => {
      avStream.addEventListener('complete', () => {
        streamDone = true;
        resolve();
      });
      avStream.addEventListener('ioError', e => reject(new Error(e.text)));
      avStream.load(new h.net.URLRequest('media/stream.m3u8'));
    });
    assert.equal(streamDone, true);
    assert.equal(avStream.connected, true);
    assert.ok(avStream.bytesAvailable > 0);
    const headerLine = avStream.readUTFBytes(7);
    assert.equal(headerLine, '#EXTM3U');
    avStream.close();
    assert.equal(avStream.connected, false);

    // AudioDecoder, AudioOutputChangeReason, AudioDeviceManager
    const AudioDecoder = B['flash.media.AudioDecoder'];
    assert.equal(AudioDecoder.DOLBY_DIGITAL, 'DolbyDigital');
    assert.equal(AudioDecoder.DOLBY_DIGITAL_PLUS, 'DolbyDigitalPlus');
    assert.equal(AudioDecoder.DTS, 'DTS');
    assert.equal(AudioDecoder.DTS_EXPRESS, 'DTSExpress');
    assert.equal(AudioDecoder.DTS_HD_HIGH_RESOLUTION_AUDIO, 'DTSHDHighResolutionAudio');
    assert.equal(AudioDecoder.DTS_HD_MASTER_AUDIO, 'DTSHDMasterAudio');

    const AudioOutputChangeReason = B['flash.media.AudioOutputChangeReason'];
    assert.equal(AudioOutputChangeReason.USER_SELECTION, 'userSelection');
    assert.equal(AudioOutputChangeReason.DEVICE_CHANGE, 'deviceChange');

    const AudioDeviceManager = B['flash.media.AudioDeviceManager'];
    assert.equal(AudioDeviceManager.isSupported, true);
    const adm = AudioDeviceManager.audioDeviceManager;
    assert.equal(adm, AudioDeviceManager.audioDeviceManager);
    assert.ok(adm.deviceNames.length >= 2);
    let outputReason = null;
    adm.addEventListener('audioOutputChange', e => {
      outputReason = e.reason;
    });
    adm.selectedDeviceIndex = 1;
    assert.equal(adm.selectedDeviceIndex, 1);
    assert.equal(outputReason, AudioOutputChangeReason.USER_SELECTION);
    assert.throws(() => {
      adm.selectedDeviceIndex = 99;
    }, RangeError);

    // Camera & CameraPosition
    const Camera = B['flash.media.Camera'];
    const CameraPosition = B['flash.media.CameraPosition'];
    assert.equal(CameraPosition.FRONT, 'front');
    assert.equal(CameraPosition.BACK, 'back');
    assert.equal(CameraPosition.UNKNOWN, 'unknown');
    assert.equal(Camera.isSupported, true);
    assert.ok(Camera.names.length >= 1);

    const cam = Camera.getCamera('0');
    assert.ok(cam);
    assert.equal(cam, Camera.getCamera('0'));
    assert.equal(Camera.getCamera('99'), null);
    assert.equal(cam.index, 0);
    assert.equal(cam.name, Camera.names[0]);
    assert.equal(cam.muted, false);
    assert.equal(cam.position, CameraPosition.FRONT);

    const camEvents = [];
    cam.addEventListener('status', e => camEvents.push(['status', e.code]));
    cam.addEventListener('activity', e => camEvents.push(['activity', e.activating]));
    cam.addEventListener('videoFrame', e => camEvents.push(['videoFrame', e.type]));

    cam.setMode(64, 48, 30, true);
    assert.equal(cam.width, 64);
    assert.equal(cam.height, 48);
    assert.equal(cam.fps, 30);
    assert.equal(cam.currentFPS, 30);

    cam.setQuality(65536, 85);
    assert.equal(cam.bandwidth, 65536);
    assert.equal(cam.quality, 85);

    cam.setKeyFrameInterval(12);
    assert.equal(cam.keyFrameInterval, 12);
    cam.setLoopback(true);
    assert.equal(cam.loopback, true);
    cam.setCursor(true);
    cam.setMotionLevel(35, 1500);
    assert.equal(cam.motionLevel, 35);
    assert.equal(cam.motionTimeout, 1500);
    assert.ok(cam.activityLevel >= 0);

    const subRect = new geom.Rectangle(0, 0, 8, 6);
    const frameBytes = new binary.ByteArray();
    cam.copyToByteArray(subRect, frameBytes);
    assert.equal(frameBytes.length, 8 * 6 * 4);
    frameBytes.position = 0;
    const firstPixelFromBa = frameBytes.readUnsignedInt();

    const frameVec = [];
    cam.copyToVector(subRect, frameVec);
    assert.equal(frameVec.length, 8 * 6);
    assert.equal(frameVec[0] >>> 0, firstPixelFromBa >>> 0);

    const destBmd = new bitmap.BitmapData(64, 48, true, 0);
    cam.drawToBitmapData(destBmd);
    assert.equal(destBmd.getPixel32(0, 0) >>> 0, firstPixelFromBa >>> 0);
    assert.ok(camEvents.some(([k]) => k === 'videoFrame'));
    assert.throws(() => {
      cam.copyToByteArray(new geom.Rectangle(60, 40, 16, 16), frameBytes);
    }, RangeError);
    destBmd.dispose();

    // Microphone, SoundCodec, MicrophoneEnhancedMode, MicrophoneEnhancedOptions
    const Microphone = B['flash.media.Microphone'];
    const SoundCodec = B['flash.media.SoundCodec'];
    const MicrophoneEnhancedMode = B['flash.media.MicrophoneEnhancedMode'];
    const MicrophoneEnhancedOptions = B['flash.media.MicrophoneEnhancedOptions'];

    assert.equal(SoundCodec.NELLYMOSER, 'NellyMoser');
    assert.equal(SoundCodec.SPEEX, 'Speex');
    assert.equal(SoundCodec.PCMA, 'pcma');
    assert.equal(SoundCodec.PCMU, 'pcmu');
    assert.equal(MicrophoneEnhancedMode.FULL_DUPLEX, 'fullDuplex');
    assert.equal(MicrophoneEnhancedMode.HALF_DUPLEX, 'halfDuplex');
    assert.equal(MicrophoneEnhancedMode.HEADSET, 'headset');
    assert.equal(MicrophoneEnhancedMode.OFF, 'off');
    assert.equal(MicrophoneEnhancedMode.SPEAKER_MUTE, 'speakerMute');

    assert.equal(Microphone.isSupported, true);
    assert.ok(Microphone.names.length >= 1);
    const mic = Microphone.getMicrophone(0);
    assert.equal(mic.index, 0);
    assert.equal(mic.name, Microphone.names[0]);
    assert.equal(mic.muted, false);
    assert.equal(mic.enhancedOptions, null);
    mic.rate = 20; // snaps to 22 kHz per Flash Player rules
    assert.equal(mic.rate, 22);
    mic.codec = SoundCodec.SPEEX;
    assert.equal(mic.codec, 'Speex');
    mic.encodeQuality = 8;
    assert.equal(mic.encodeQuality, 8);
    mic.framesPerPacket = 3;
    assert.equal(mic.framesPerPacket, 3);
    mic.gain = 75;
    assert.equal(mic.gain, 75);
    mic.enableVAD = true;
    assert.equal(mic.enableVAD, true);
    mic.noiseSuppressionLevel = -20;
    assert.equal(mic.noiseSuppressionLevel, -20);
    mic.setLoopBack(true);
    mic.setSilenceLevel(15, 2500);
    assert.equal(mic.silenceLevel, 15);
    assert.equal(mic.silenceTimeout, 2500);
    assert.ok(mic.activityLevel >= 0);
    mic.setUseEchoSuppression(true);
    assert.equal(mic.useEchoSuppression, true);
    mic.soundTransform = new h.audio.SoundTransform(0.6, -0.25);
    assert.ok(Math.abs(mic.soundTransform.volume - 0.6) < 1e-6);

    const emic = Microphone.getEnhancedMicrophone(0);
    assert.ok(MicrophoneEnhancedOptions.__nfIs(emic.enhancedOptions));
    const opts = new MicrophoneEnhancedOptions();
    opts.mode = MicrophoneEnhancedMode.HALF_DUPLEX;
    opts.echoPath = 256;
    opts.nonLinearProcessing = false;
    opts.isVoiceDetected = 1;
    emic.enhancedOptions = opts;
    assert.equal(emic.enhancedOptions.mode, 'halfDuplex');
    assert.equal(emic.enhancedOptions.echoPath, 256);
    assert.equal(emic.enhancedOptions.nonLinearProcessing, false);
    assert.equal(emic.enhancedOptions.isVoiceDetected, 1);
    assert.throws(() => {
      opts.echoPath = 64;
    }, /ArgumentError|echoPath/);

    // VideoCodec, H264Profile, H264Level, VideoStreamSettings, H264VideoStreamSettings, VP6VideoStreamSettings
    const VideoCodec = B['flash.media.VideoCodec'];
    const H264Profile = B['flash.media.H264Profile'];
    const H264Level = B['flash.media.H264Level'];
    const VideoStreamSettings = B['flash.media.VideoStreamSettings'];
    const H264VideoStreamSettings = B['flash.media.H264VideoStreamSettings'];
    const VP6VideoStreamSettings = B['flash.media.VP6VideoStreamSettings'];

    assert.equal(VideoCodec.H264AVC, 'H264Avc');
    assert.equal(VideoCodec.SORENSON, 'Sorenson');
    assert.equal(H264Profile.BASELINE, 'baseline');
    assert.equal(H264Profile.MAIN, 'main');
    for (const [k, expected] of [
      ['LEVEL_1', '1'],
      ['LEVEL_1B', '1b'],
      ['LEVEL_1_1', '1.1'],
      ['LEVEL_1_2', '1.2'],
      ['LEVEL_1_3', '1.3'],
      ['LEVEL_2', '2'],
      ['LEVEL_2_1', '2.1'],
      ['LEVEL_2_2', '2.2'],
      ['LEVEL_3', '3'],
      ['LEVEL_3_1', '3.1'],
      ['LEVEL_3_2', '3.2'],
      ['LEVEL_4', '4'],
      ['LEVEL_4_1', '4.1'],
      ['LEVEL_4_2', '4.2'],
      ['LEVEL_5', '5'],
      ['LEVEL_5_1', '5.1'],
    ]) {
      assert.equal(H264Level[k], expected);
    }

    const vss = new VideoStreamSettings();
    assert.equal(vss.codec, 'Sorenson');
    vss.setMode(640, 360, 25);
    vss.setQuality(128000, 90);
    vss.setKeyFrameInterval(24);
    assert.equal(vss.width, 640);
    assert.equal(vss.height, 360);
    assert.equal(vss.fps, 25);
    assert.equal(vss.bandwidth, 128000);
    assert.equal(vss.quality, 90);
    assert.equal(vss.keyFrameInterval, 24);

    const h264 = new H264VideoStreamSettings();
    assert.ok(h264 instanceof VideoStreamSettings);
    assert.equal(h264.codec, 'H264Avc');
    assert.equal(h264.profile, 'baseline');
    assert.equal(h264.level, '2.1');
    h264.setProfileLevel(H264Profile.MAIN, H264Level.LEVEL_4_1);
    assert.equal(h264.profile, 'main');
    assert.equal(h264.level, '4.1');
    assert.throws(() => {
      h264.setProfileLevel('high10', '9.9');
    }, /profile|ArgumentError/);

    const vp6 = new VP6VideoStreamSettings();
    assert.ok(vp6 instanceof VideoStreamSettings);
    assert.equal(vp6.codec, 'VP6');

    // StageVideo, StageVideoAvailability, StageVideoAvailabilityReason, Stage.stageVideos
    const StageVideo = B['flash.media.StageVideo'];
    const StageVideoAvailability = B['flash.media.StageVideoAvailability'];
    const StageVideoAvailabilityReason = B['flash.media.StageVideoAvailabilityReason'];
    assert.equal(StageVideoAvailability.AVAILABLE, 'available');
    assert.equal(StageVideoAvailability.UNAVAILABLE, 'unavailable');
    assert.equal(StageVideoAvailabilityReason.NO_ERROR, 'noError');
    assert.equal(StageVideoAvailabilityReason.DRIVER_TOO_OLD, 'driverTooOld');
    assert.equal(StageVideoAvailabilityReason.UNAVAILABLE, 'unavailable');
    assert.equal(StageVideoAvailabilityReason.USER_DISABLED, 'userDisabled');
    assert.equal(StageVideoAvailabilityReason.WMODE_INCOMPATIBLE, 'wmodeIncompatible');

    const svVec = stage.stageVideos;
    assert.equal(svVec.length, 4);
    const sv = svVec[0];
    assert.ok(StageVideo.__nfIs(sv));
    sv.viewPort = new geom.Rectangle(10, 20, 640, 360);
    assert.equal(sv.viewPort.width, 640);
    sv.depth = 3;
    assert.equal(sv.depth, 3);
    sv.pan = new geom.Point(-0.5, 0.25);
    assert.equal(sv.pan.x, -0.5);
    sv.zoom = new geom.Point(2, 2);
    assert.equal(sv.zoom.x, 2);
    assert.ok(sv.colorSpaces.includes('BT.709'));
    assert.throws(() => {
      sv.pan = new geom.Point(5, 0);
    }, RangeError);
    assert.throws(() => {
      sv.zoom = new geom.Point(0.5, 1);
    }, RangeError);

    let renderStatus = null;
    sv.addEventListener('renderState', e => {
      renderStatus = e.status;
    });
    sv.attachCamera(cam);
    assert.equal(sv.videoWidth, 64);
    assert.equal(sv.videoHeight, 48);
    assert.equal(renderStatus, 'software');
    sv.attachNetStream({ videoWidth: 1280, videoHeight: 720 });
    assert.equal(sv.videoWidth, 1280);
    assert.equal(sv.videoHeight, 720);
    sv.attachNetStream(null);
    assert.equal(sv.videoWidth, 0);
  } finally {
    h.dispose();
  }
});

test('flash.ui.* ContextMenu, Multitouch, GameInput & flash.accessibility.* classes satisfy AS3 contracts', async () => {
  await ready;
  const h = createMediaEventsHarness({ accessibilityActive: true });
  try {
    const { B, binary, geom, air } = h;

    // ContextMenuBuiltInItems, ContextMenuClipboardItems, ContextMenuItem, ContextMenu
    const ContextMenu = B['flash.ui.ContextMenu'];
    const ContextMenuItem = B['flash.ui.ContextMenuItem'];
    const ContextMenuBuiltInItems = B['flash.ui.ContextMenuBuiltInItems'];
    const ContextMenuClipboardItems = B['flash.ui.ContextMenuClipboardItems'];

    assert.equal(ContextMenu.isSupported, true);
    const cm = new ContextMenu();
    assert.ok(cm instanceof air.builtins['flash.display.NativeMenu']);
    assert.ok(ContextMenuBuiltInItems.__nfIs(cm.builtInItems));
    assert.equal(cm.builtInItems.play, true);
    assert.equal(cm.builtInItems.zoom, true);
    cm.hideBuiltInItems();
    for (const k of ['forwardAndBack', 'loop', 'play', 'print', 'quality', 'rewind', 'save', 'zoom']) {
      assert.equal(cm.builtInItems[k], false, `hideBuiltInItems must clear ${k}`);
    }
    assert.ok(ContextMenuClipboardItems.__nfIs(cm.clipboardItems));
    cm.clipboardMenu = true;
    cm.clipboardItems.paste = true;
    cm.clipboardItems.cut = true;
    assert.equal(cm.clipboardMenu, true);
    assert.equal(cm.clipboardItems.paste, true);

    const req = new h.net.URLRequest('https://neoflash.example');
    cm.link = req;
    assert.equal(cm.link, req);

    const item = new ContextMenuItem('Inspect Element', true, true, true);
    assert.ok(item instanceof air.builtins['flash.display.NativeMenuItem']);
    assert.equal(item.caption, 'Inspect Element');
    assert.equal(item.separatorBefore, true);
    assert.equal(item.enabled, true);
    assert.equal(item.visible, true);
    item.caption = 'Inspect Frame';
    assert.equal(item.label, 'Inspect Frame');
    cm.customItems = [item];
    assert.equal(cm.customItems.length, 1);

    let itemSelected = false;
    item.addEventListener('menuItemSelect', () => {
      itemSelected = true;
    });
    item.dispatchEvent(new h.E.ContextMenuEvent('menuItemSelect'));
    assert.equal(itemSelected, true);

    const clonedMenu = cm.clone();
    assert.equal(clonedMenu.clipboardMenu, true);
    assert.equal(clonedMenu.customItems.length, 1);
    assert.notEqual(clonedMenu.customItems[0], item);
    assert.equal(clonedMenu.customItems[0].caption, 'Inspect Frame');

    // Multitouch & MultitouchInputMode
    const Multitouch = B['flash.ui.Multitouch'];
    const MultitouchInputMode = B['flash.ui.MultitouchInputMode'];
    assert.equal(MultitouchInputMode.GESTURE, 'gesture');
    assert.equal(MultitouchInputMode.NONE, 'none');
    assert.equal(MultitouchInputMode.TOUCH_POINT, 'touchPoint');
    assert.equal(Multitouch.supportsGestureEvents, true);
    assert.equal(Multitouch.supportsTouchEvents, true);
    assert.ok(Multitouch.maxTouchPoints >= 2);
    assert.ok(Multitouch.supportedGestures.includes('gestureZoom'));
    Multitouch.inputMode = MultitouchInputMode.TOUCH_POINT;
    assert.equal(Multitouch.inputMode, 'touchPoint');
    Multitouch.mapTouchToMouse = false;
    assert.equal(Multitouch.mapTouchToMouse, false);
    assert.throws(() => {
      Multitouch.inputMode = 'invalidMode';
    }, /ArgumentError|Multitouch/);

    // GameInput, GameInputDevice, GameInputControl
    const GameInput = B['flash.ui.GameInput'];
    const GameInputDevice = B['flash.ui.GameInputDevice'];
    const GameInputControl = B['flash.ui.GameInputControl'];
    assert.equal(GameInput.isSupported, true);
    assert.equal(GameInput.numDevices, 1);
    const gi = new GameInput();
    assert.ok(gi);
    const dev = GameInput.getDeviceAt(0);
    assert.ok(GameInputDevice.__nfIs(dev));
    assert.equal(GameInputDevice.MAX_BUFFER_SIZE, 32000);
    assert.ok(dev.numControls >= 4);
    dev.enabled = true;
    assert.equal(dev.enabled, true);
    const ctrl = dev.getControlAt(0);
    assert.ok(GameInputControl.__nfIs(ctrl));
    assert.equal(ctrl.device, dev);
    assert.equal(ctrl.minValue, -1);
    assert.equal(ctrl.maxValue, 1);
    dev.startCachingSamples(16, [ctrl.id]);
    const cachedBa = new binary.ByteArray();
    const numSamples = dev.getCachedSamples(cachedBa, false);
    assert.equal(numSamples, 1);
    assert.equal(cachedBa.length, 4);
    dev.stopCachingSamples();

    // Accessibility, AccessibilityProperties, AccessibilityImplementation, ISearchableText, ISimpleTextSelection
    const Accessibility = B['flash.accessibility.Accessibility'];
    const AccessibilityProperties = B['flash.accessibility.AccessibilityProperties'];
    const AccessibilityImplementation = B['flash.accessibility.AccessibilityImplementation'];
    const ISearchableText = B['flash.accessibility.ISearchableText'];
    const ISimpleTextSelection = B['flash.accessibility.ISimpleTextSelection'];

    assert.equal(Accessibility.active, true);
    h.setAccActive(false);
    assert.equal(Accessibility.active, false);
    Accessibility.updateProperties();
    assert.equal(h.getAccUpdateCount(), 1);
    Accessibility.sendEvent({}, 0, 0x8005, true);
    assert.equal(h.getAccEvents().length, 1);
    assert.equal(h.getAccEvents()[0].eventType, 0x8005);

    const ap = new AccessibilityProperties();
    assert.equal(ap.name, '');
    assert.equal(ap.description, '');
    assert.equal(ap.shortcut, '');
    assert.equal(ap.silent, false);
    assert.equal(ap.forceSimple, false);
    assert.equal(ap.noAutoLabeling, false);
    ap.name = 'Play Button';
    ap.description = 'Starts playback';
    ap.shortcut = 'Ctrl+P';
    ap.silent = false;
    ap.forceSimple = true;
    ap.noAutoLabeling = true;
    assert.equal(ap.name, 'Play Button');
    assert.equal(ap.forceSimple, true);
    assert.equal(ap.noAutoLabeling, true);

    const baseImpl = new AccessibilityImplementation();
    assert.equal(baseImpl.errno, 0);
    assert.equal(baseImpl.stub, false);
    baseImpl.errno = 2;
    baseImpl.stub = true;
    assert.equal(baseImpl.errno, 2);
    assert.equal(baseImpl.stub, true);
    assert.equal(baseImpl.getChildIDArray(), null);
    assert.equal(baseImpl.get_accFocus(), 0);
    assert.equal(baseImpl.get_accSelection(), null);
    assert.equal(baseImpl.isLabeledBy(new geom.Rectangle(0, 0, 10, 10)), false);
    assert.throws(() => baseImpl.get_accRole(0), err => err.errorID === 2143);
    assert.throws(() => baseImpl.get_accState(0), err => err.errorID === 2144);

    const searchableCandidate = {
      searchText: 'hello world',
      selectionActiveIndex: 5,
      selectionAnchorIndex: 0,
    };
    assert.equal(ISearchableText.__nfIs(searchableCandidate), true);
    assert.equal(ISimpleTextSelection.__nfIs(searchableCandidate), true);
  } finally {
    h.dispose();
  }
});

test('runtime host wires DisplayObject.accessibilityProperties, Stage.accessibilityProperties (#2071), Video.attachCamera, NetStatusEvent.toString, and SoundMixer.areSoundsInaccessible', async () => {
  await ready;
  const root = path.resolve(__dirname, '../..');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const dummyProject = {
    format_version: 3,
    name: 'Host Integration Media Check',
    width: 640,
    height: 480,
    fps: 24,
    background: [0, 0, 0, 255],
    root: 'root',
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'l1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, elements: [], label: '', tween: null }],
            },
          ],
          scripts: [],
        },
      },
    ],
  };

  const player = new wasm.WebPlayer(JSON.stringify(dummyProject));
  const { host } = createRuntimeHost(root, player, [], { project: dummyProject });
  try {
    const prog = host.program();
    const Sprite = prog.type('flash.display.Sprite');
    const Video = prog.type('flash.media.Video');
    const Camera = prog.type('flash.media.Camera');
    const SoundMixer = prog.type('flash.media.SoundMixer');
    const NetStatusEvent = prog.type('flash.events.NetStatusEvent');
    const AccessibilityProperties = prog.type('flash.accessibility.AccessibilityProperties');
    const Accessibility = prog.type('flash.accessibility.Accessibility');

    assert.equal(SoundMixer.areSoundsInaccessible(), false);

    const nse = new NetStatusEvent(NetStatusEvent.NET_STATUS, false, false, {
      code: 'NetStream.Play.Start',
      level: 'status',
    });
    assert.equal(nse.clone().info.code, 'NetStream.Play.Start');
    assert.match(nse.toString(), /^\[NetStatusEvent type="netStatus"/);

    const sp = new Sprite();
    assert.equal(sp.accessibilityProperties, null);
    const props = new AccessibilityProperties();
    props.name = 'Hero Sprite';
    props.description = 'Interactive character';
    sp.accessibilityProperties = props;
    assert.equal(sp.accessibilityProperties, props);
    const rootClip = host.proxyFor([]);
    rootClip.addChild(sp);
    Accessibility.updateProperties();

    // Stage.accessibilityProperties throws IllegalOperationError #2071 on write
    assert.equal(rootClip.stage.accessibilityProperties, null);
    assert.throws(() => {
      rootClip.stage.accessibilityProperties = props;
    }, /2071|IllegalOperationError|accessibility/);

    // Stage.stageVideos returns 4 StageVideo slots
    assert.equal(rootClip.stage.stageVideos.length, 4);

    // Video.attachCamera attaches and detaches Camera cleanly
    const vid = new Video(320, 240);
    const cam = Camera.getCamera('0');
    cam.setMode(160, 120, 24, true);
    vid.attachCamera(cam);
    assert.equal(vid.videoWidth, 160);
    assert.equal(vid.videoHeight, 120);
    vid.attachCamera(null);
    assert.equal(vid.videoWidth, 0);
    assert.equal(vid.videoHeight, 0);
  } finally {
    host.disposeFamily();
    player.free();
  }
});

test('Wave 6: interactive HTML5 DOM ContextMenu renders custom, clipboard, and built-in items and dispatches MENU_SELECT + MENU_ITEM_SELECT', async () => {
  await ready;
  const h = createMediaEventsHarness();
  try {
    const { mediaEvents: host, B, E } = h;
    const ContextMenu = B['flash.ui.ContextMenu'];
    const ContextMenuItem = B['flash.ui.ContextMenuItem'];

    const target = new E.EventDispatcher();
    target.text = 'Hello Flash World';
    target.selectionBeginIndex = 6;
    target.selectionEndIndex = 11;

    const cm = new ContextMenu();
    cm.hideBuiltInItems();
    cm.builtInItems.play = true;
    cm.clipboardItems.copy = true;
    cm.clipboardItems.cut = true;
    cm.clipboardItems.paste = true;
    cm.clipboardItems.clear = true;
    cm.clipboardItems.selectAll = true;

    const item1 = new ContextMenuItem('Inspect Symbol', false, true, true);
    const item2 = new ContextMenuItem('Disabled Action', true, false, true);
    const itemHidden = new ContextMenuItem('Secret Action', false, true, false);
    cm.customItems = [item1, item2, itemHidden];
    target.contextMenu = cm;

    const menuSelectEvents = [];
    const itemSelectEvents = [];
    cm.addEventListener(E.ContextMenuEvent.MENU_SELECT, ev => menuSelectEvents.push(ev));
    item1.addEventListener(E.ContextMenuEvent.MENU_ITEM_SELECT, ev => itemSelectEvents.push(ev));

    let prevented = false;
    const domMenu = host.openContextMenu(
      {
        clientX: 120,
        clientY: 80,
        preventDefault() {
          prevented = true;
        },
      },
      target
    );
    assert.equal(prevented, true);
    assert.ok(domMenu);
    assert.equal(domMenu.getAttribute('role'), 'menu');
    assert.equal(menuSelectEvents.length, 1);
    assert.equal(menuSelectEvents[0].mouseTarget, target);
    assert.equal(menuSelectEvents[0].contextMenuOwner, target);

    const buttons = domMenu.querySelectorAll('[role="menuitem"]');
    const captions = buttons.map(b => b.textContent);
    assert.ok(captions.includes('Inspect Symbol'));
    assert.ok(captions.includes('Disabled Action'));
    assert.ok(!captions.includes('Secret Action'));
    assert.ok(captions.includes('Cut'));
    assert.ok(captions.includes('Copy'));
    assert.ok(captions.includes('Paste'));
    assert.ok(captions.includes('Clear'));
    assert.ok(captions.includes('Select All'));
    assert.ok(captions.includes('Play'));

    const separators = domMenu.querySelectorAll('[role="separator"]');
    assert.ok(separators.length >= 2);

    // Click disabled item -> should NOT fire MENU_ITEM_SELECT
    const disabledBtn = buttons.find(b => b.textContent === 'Disabled Action');
    assert.equal(disabledBtn.disabled, true);
    disabledBtn.click();
    assert.equal(itemSelectEvents.length, 0);

    // Click enabled custom item -> fires MENU_ITEM_SELECT and closes menu
    const inspectBtn = buttons.find(b => b.textContent === 'Inspect Symbol');
    inspectBtn.click();
    assert.equal(itemSelectEvents.length, 1);
    assert.equal(itemSelectEvents[0].mouseTarget, target);
    assert.equal(itemSelectEvents[0].contextMenuOwner, target);
    assert.equal(host.getActiveContextMenu(), null);

    // Reopen and test clipboard Cut + Paste + Select All on target
    const menu2 = host.openContextMenu({ clientX: 10, clientY: 10 }, target);
    const cutBtn = menu2.querySelectorAll('[role="menuitem"]').find(b => b.textContent === 'Cut');
    cutBtn.click();
    assert.equal(target.text, 'Hello  World');

    target.selectionBeginIndex = 6;
    target.selectionEndIndex = 6;
    const menu3 = host.openContextMenu({ clientX: 10, clientY: 10 }, target);
    const pasteBtn = menu3.querySelectorAll('[role="menuitem"]').find(b => b.textContent === 'Paste');
    pasteBtn.click();
    assert.equal(target.text, 'Hello Flash World');

    const menu4 = host.openContextMenu({ clientX: 10, clientY: 10 }, target);
    const selectAllBtn = menu4.querySelectorAll('[role="menuitem"]').find(b => b.textContent === 'Select All');
    selectAllBtn.click();
    assert.equal(target.selectionBeginIndex, 0);
    assert.equal(target.selectionEndIndex, target.text.length);
  } finally {
    h.dispose();
  }
});

test('Wave 6: Multitouch recognizer dispatches multi-pointer TouchEvent and 2-finger TransformGestureEvent / GestureEvent / PressAndTapGestureEvent', async () => {
  await ready;
  const h = createMediaEventsHarness();
  try {
    const { mediaEvents: host, B, E, binary } = h;
    const Multitouch = B['flash.ui.Multitouch'];
    const MultitouchInputMode = B['flash.ui.MultitouchInputMode'];

    const targetA = new E.EventDispatcher();
    const targetB = new E.EventDispatcher();

    // 1. TOUCH_POINT mode
    Multitouch.inputMode = MultitouchInputMode.TOUCH_POINT;
    const touchLog = [];
    for (const t of [
      E.TouchEvent.TOUCH_BEGIN,
      E.TouchEvent.TOUCH_MOVE,
      E.TouchEvent.TOUCH_END,
      E.TouchEvent.TOUCH_TAP,
      E.TouchEvent.TOUCH_OVER,
      E.TouchEvent.TOUCH_OUT,
    ]) {
      targetA.addEventListener(t, ev => touchLog.push({ target: 'A', type: ev.type, id: ev.touchPointID, primary: ev.isPrimaryTouchPoint, ev }));
      targetB.addEventListener(t, ev => touchLog.push({ target: 'B', type: ev.type, id: ev.touchPointID, primary: ev.isPrimaryTouchPoint, ev }));
    }

    host.handlePointerInput('pointerdown', { pointerId: 10, stageX: 50, stageY: 60, pressure: 0.75 }, targetA);
    host.handlePointerInput('pointermove', { pointerId: 10, stageX: 52, stageY: 61, pressure: 0.8 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 10, stageX: 52, stageY: 61, pressure: 0.8 }, targetA);

    assert.ok(touchLog.some(e => e.type === 'touchBegin' && e.id === 10 && e.primary === true));
    const moveEntry = touchLog.find(e => e.type === 'touchMove' && e.id === 10);
    assert.ok(moveEntry);
    const sampleBuf = new binary.ByteArray();
    const numSamples = moveEntry.ev.getSamples(sampleBuf, false);
    assert.equal(numSamples, 2);
    assert.equal(sampleBuf.length, 24);
    assert.ok(touchLog.some(e => e.type === 'touchEnd' && e.id === 10));
    assert.ok(touchLog.some(e => e.type === 'touchTap' && e.id === 10));

    // Pointer transition from targetA to targetB dispatches touchOut on A and touchOver on B
    touchLog.length = 0;
    host.handlePointerInput('pointerdown', { pointerId: 20, stageX: 10, stageY: 10 }, targetA);
    host.handlePointerInput('pointermove', { pointerId: 20, stageX: 100, stageY: 100 }, targetB);
    host.handlePointerInput('pointerup', { pointerId: 20, stageX: 100, stageY: 100 }, targetB);
    assert.ok(touchLog.some(e => e.target === 'A' && e.type === 'touchOut'));
    assert.ok(touchLog.some(e => e.target === 'B' && e.type === 'touchOver'));

    // 2. GESTURE mode: 2-finger pinch zoom + rotate + pan, 2-finger tap, press-and-tap, and 1-finger swipe
    Multitouch.inputMode = MultitouchInputMode.GESTURE;
    const gestureLog = [];
    for (const g of [
      E.TransformGestureEvent.GESTURE_ZOOM,
      E.TransformGestureEvent.GESTURE_ROTATE,
      E.TransformGestureEvent.GESTURE_PAN,
      E.TransformGestureEvent.GESTURE_SWIPE,
      E.GestureEvent.GESTURE_TWO_FINGER_TAP,
      E.PressAndTapGestureEvent.GESTURE_PRESS_AND_TAP,
    ]) {
      targetA.addEventListener(g, ev => gestureLog.push(ev));
    }

    // Pinch zoom + rotate + pan
    host.handlePointerInput('pointerdown', { pointerId: 1, stageX: 100, stageY: 100 }, targetA);
    host.handlePointerInput('pointerdown', { pointerId: 2, stageX: 200, stageY: 100 }, targetA);
    host.handlePointerInput('pointermove', { pointerId: 2, stageX: 250, stageY: 150 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 2, stageX: 250, stageY: 150 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 1, stageX: 100, stageY: 100 }, targetA);

    assert.ok(gestureLog.some(e => e.type === 'gestureZoom' && e.scaleX > 1));
    assert.ok(gestureLog.some(e => e.type === 'gestureRotate' && e.rotation !== 0));
    assert.ok(gestureLog.some(e => e.type === 'gesturePan'));

    // Two-finger tap
    gestureLog.length = 0;
    host.handlePointerInput('pointerdown', { pointerId: 3, stageX: 100, stageY: 100 }, targetA);
    host.handlePointerInput('pointerdown', { pointerId: 4, stageX: 140, stageY: 100 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 4, stageX: 140, stageY: 100 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 3, stageX: 100, stageY: 100 }, targetA);
    assert.ok(gestureLog.some(e => e.type === 'gestureTwoFingerTap'));

    // Press and tap
    gestureLog.length = 0;
    host.handlePointerInput('pointerdown', { pointerId: 5, stageX: 80, stageY: 90 }, targetA);
    host.handlePointerInput('pointerdown', { pointerId: 6, stageX: 120, stageY: 95 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 6, stageX: 120, stageY: 95, pressAndTap: true }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 5, stageX: 80, stageY: 90 }, targetA);
    assert.ok(gestureLog.some(e => e.type === 'gesturePressAndTap' && e.tapLocalX === 120));

    // One-finger swipe
    gestureLog.length = 0;
    host.handlePointerInput('pointerdown', { pointerId: 7, stageX: 50, stageY: 50 }, targetA);
    host.handlePointerInput('pointermove', { pointerId: 7, stageX: 150, stageY: 55 }, targetA);
    host.handlePointerInput('pointerup', { pointerId: 7, stageX: 150, stageY: 55 }, targetA);
    assert.ok(gestureLog.some(e => e.type === 'gestureSwipe' && e.offsetX === 1));
  } finally {
    h.dispose();
  }
});

test('Wave 6: GameInput live gamepad sync, StageVideo canvas compositing, Microphone PCM SampleDataEvent, scanHardware, and SemanticLayer AccessibilityProperties', async () => {
  await ready;
  const h = createMediaEventsHarness();
  try {
    const { mediaEvents: host, B, E, binary, geom, stage: mockStage } = h;

    // 1. GameInput live gamepad sync & events
    const GameInput = B['flash.ui.GameInput'];
    const GameInputEvent = B['flash.events.GameInputEvent'];
    const gi = new GameInput();
    const addedDevices = [];
    const removedDevices = [];
    gi.addEventListener(GameInputEvent.DEVICE_ADDED, ev => addedDevices.push(ev.device));
    gi.addEventListener(GameInputEvent.DEVICE_REMOVED, ev => removedDevices.push(ev.device));

    const dev0 = GameInput.getDeviceAt(0);
    dev0.enabled = true;
    dev0.startCachingSamples(8, ['AXIS_0', 'BUTTON_0']);
    const axis0 = dev0.getControlAt(0);
    let axisChangeCount = 0;
    axis0.addEventListener('change', () => {
      axisChangeCount++;
    });

    host.syncGamepads([
      { id: 'xbox-0', name: 'Xbox Wireless Controller', axes: [0.85, -0.5], buttons: [{ value: 1, pressed: true }] },
      { id: 'dualshock-1', name: 'DualSense Wireless Controller', axes: [0.0, 0.25], buttons: [0] },
    ]);
    assert.equal(GameInput.numDevices, 2);
    assert.equal(addedDevices.length, 1);
    assert.equal(addedDevices[0].id, 'dualshock-1');
    assert.equal(axisChangeCount, 1);
    assert.ok(Math.abs(axis0.value - 0.85) < 1e-5);

    const sampleBytes = new binary.ByteArray();
    const cachedCount = dev0.getCachedSamples(sampleBytes, false);
    assert.equal(cachedCount, 2); // initial snapshot + synced sample
    assert.equal(sampleBytes.length, 16);

    host.removeGamepad(1);
    assert.equal(GameInput.numDevices, 1);
    assert.equal(removedDevices.length, 1);

    // 2. StageVideo canvas compositing & StageVideoAvailabilityEvent on stage
    let availReceived = null;
    mockStage.addEventListener('stageVideoAvailability', ev => {
      availReceived = ev;
    });
    assert.ok(availReceived);
    assert.equal(availReceived.availability, 'available');

    const sv0 = mockStage.stageVideos[0];
    const sv1 = mockStage.stageVideos[1];
    const Camera = B['flash.media.Camera'];
    const cam = Camera.getCamera('0');
    cam.setMode(320, 240, 30, true);
    sv0.viewPort = new geom.Rectangle(10, 20, 400, 300);
    sv0.depth = 5;
    sv0.attachCamera(cam);

    sv1.viewPort = new geom.Rectangle(0, 0, 800, 600);
    sv1.depth = 1;
    sv1.attachNetStream({ videoWidth: 640, videoHeight: 360 });

    const drawCalls = [];
    const mockCtx = {
      fillStyle: '#000000',
      save() {},
      restore() {},
      beginPath() {},
      rect() {},
      clip() {},
      fillRect(x, y, w, h) {
        drawCalls.push({ x, y, w, h, fillStyle: this.fillStyle });
      },
    };
    const compositeSummary = host.paintStageVideos(mockCtx, 800, 600);
    assert.equal(compositeSummary.length, 2);
    // Sorted by depth ascending: slot 1 (depth 1) painted before slot 0 (depth 5)
    assert.equal(compositeSummary[0].slotIndex, 1);
    assert.equal(compositeSummary[1].slotIndex, 0);
    assert.equal(drawCalls.length, 2);

    // 3. Camera live video source override
    cam.attachVideoSource({
      getPixel32(x, y) {
        return (0xFF112200 | ((x + y) & 0xFF)) >>> 0;
      },
    });
    const customVec = [];
    cam.copyToVector(new geom.Rectangle(0, 0, 2, 2), customVec);
    assert.equal(customVec[0], 0xFF112200);
    assert.equal(customVec[1], 0xFF112201);
    cam.attachVideoSource(null);

    // 4. Microphone SampleDataEvent PCM float32 capture & scanHardware
    const Microphone = B['flash.media.Microphone'];
    const mic = Microphone.getMicrophone(0);
    mic.rate = 44;
    mic.gain = 75;
    const micSamples = [];
    mic.addEventListener(E.SampleDataEvent.SAMPLE_DATA, ev => {
      micSamples.push({ position: ev.position, length: ev.data.length, firstSample: ev.data.readFloat() });
    });
    host.stepMicrophones(256);
    assert.equal(micSamples.length, 1);
    assert.equal(micSamples[0].position, 0);
    assert.equal(micSamples[0].length, 1024);
    assert.ok(Number.isFinite(micSamples[0].firstSample));
    assert.ok(mic.activityLevel > 0);

    const AudioDeviceManager = B['flash.media.AudioDeviceManager'];
    const adm = AudioDeviceManager.audioDeviceManager;
    let deviceChangeReason = null;
    adm.addEventListener('audioOutputChange', ev => {
      deviceChangeReason = ev.reason;
    });
    const hw = B['flash.media.scanHardware']({
      cameras: ['Studio Camera Pro'],
      microphones: ['Studio Condenser Mic'],
      audioOutputs: ['Studio Monitors', 'Headphones Out'],
    });
    assert.deepEqual(Array.from(Camera.names), ['Studio Camera Pro']);
    assert.deepEqual(Array.from(Microphone.names), ['Studio Condenser Mic']);
    assert.deepEqual(Array.from(adm.deviceNames), ['Studio Monitors', 'Headphones Out']);
    assert.equal(deviceChangeReason, 'deviceChange');
    assert.ok(hw);

    // 5. SemanticLayer AccessibilityProperties (shortcut, silent, forceSimple, noAutoLabeling)
    const AccessibilityLib = require('../accessibility.js');
    const AccessibilityProperties = B['flash.accessibility.AccessibilityProperties'];
    const domRecords = [];
    const fakeDoc = {
      activeElement: null,
      createElement(tag) {
        const attrs = new Map();
        const children = [];
        return {
          tagName: tag.toUpperCase(),
          className: '',
          type: '',
          hidden: false,
          id: '',
          disabled: false,
          textContent: '',
          style: {},
          children,
          setAttribute(k, v) { attrs.set(k, String(v)); },
          getAttribute(k) { return attrs.has(k) ? attrs.get(k) : null; },
          removeAttribute(k) { attrs.delete(k); },
          appendChild(c) { children.push(c); return c; },
          addEventListener() {},
          remove() {
            const idx = domRecords.indexOf(this);
            if (idx >= 0) domRecords.splice(idx, 1);
          },
          get nextElementSibling() {
            const idx = domRecords.indexOf(this);
            return idx >= 0 ? domRecords[idx + 1] || null : null;
          },
        };
      },
    };
    const fakeContainer = {
      ownerDocument: fakeDoc,
      get firstElementChild() { return domRecords[0] || null; },
      insertBefore(el, before) {
        const existing = domRecords.indexOf(el);
        if (existing >= 0) domRecords.splice(existing, 1);
        const idx = before ? domRecords.indexOf(before) : -1;
        if (idx >= 0) domRecords.splice(idx, 0, el);
        else domRecords.push(el);
      },
    };
    const semanticLayer = new AccessibilityLib.SemanticLayer(fakeContainer, () => {});
    const dispMap = new Map();
    const pParent = new AccessibilityProperties();
    pParent.name = 'Toolbar Container';
    pParent.description = 'Main toolbar';
    pParent.shortcut = 'Ctrl+T';
    pParent.forceSimple = true;
    pParent.noAutoLabeling = true;
    dispMap.set('parent', { accessibilityProperties: pParent });

    const pChild = new AccessibilityProperties();
    pChild.name = 'Suppressed Child';
    dispMap.set('child', { accessibilityProperties: pChild });

    const pSilent = new AccessibilityProperties();
    pSilent.name = 'Hidden Node';
    pSilent.silent = true;
    dispMap.set('silent', { accessibilityProperties: pSilent });

    const rawNodes = [
      {
        instance_path: ['toolbar'],
        lifetime_id: 1,
        key: 'parent',
        accessibility: { role: 'button', name: 'Old', description: '' },
      },
      {
        instance_path: ['toolbar', 'innerBtn'],
        lifetime_id: 2,
        key: 'child',
        accessibility: { role: 'button', name: 'Inner', description: '' },
      },
      {
        instance_path: ['ghost'],
        lifetime_id: 3,
        key: 'silent',
        accessibility: { role: 'button', name: 'Ghost', description: '' },
      },
    ];
    const enriched = host.applyAccessibilityPropertiesToSemanticNodes(rawNodes, n => dispMap.get(n.key));
    const sceneItems = [
      {
        instance_path: ['toolbar'],
        alpha: 1,
        transform: [1, 0, 0, 1, 0, 0],
        kind: { type: 'rectangle', width: 100, height: 40, fill: [255, 0, 0, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
      },
      {
        instance_path: ['toolbar', 'innerBtn'],
        alpha: 1,
        transform: [1, 0, 0, 1, 10, 5],
        kind: { type: 'rectangle', width: 30, height: 20, fill: [0, 255, 0, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
      },
      {
        instance_path: ['ghost'],
        alpha: 1,
        transform: [1, 0, 0, 1, 200, 5],
        kind: { type: 'rectangle', width: 30, height: 20, fill: [0, 0, 255, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
      },
    ];
    const count = semanticLayer.update(enriched, sceneItems, { width: 800, height: 600 }, 1, () => 20);
    // Only 'toolbar' remains: 'silent' was omitted by silent=true, and 'innerBtn' was suppressed by parent forceSimple=true
    assert.equal(count, 1);
    assert.equal(domRecords.length, 1);
    assert.equal(domRecords[0].getAttribute('aria-label'), 'Toolbar Container');
    assert.equal(domRecords[0].getAttribute('aria-keyshortcuts'), 'Ctrl+T');
    assert.equal(domRecords[0].getAttribute('data-force-simple'), 'true');
    assert.equal(domRecords[0].getAttribute('data-no-auto-labeling'), 'true');
  } finally {
    h.dispose();
  }
});
