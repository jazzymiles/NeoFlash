/* Stage/input adapter for the shared event hub. Logical dimensions are supplied by the host;
 * browser preview zoom must not silently change stage coordinates. Fullscreen, IME, touch
 * gestures, Stage3D and Flash-specific keyboard defaults remain outside this adapter.
 */
(function (global) {
  'use strict';
  const eventsAPI = global.NeoFlashEvents || (typeof require === 'function' ? require('./events.js') : null);
  const stageToken = Symbol('stage host construction');
  const stages = new WeakSet();
  class Stage extends (eventsAPI?.EventDispatcher || class {}) {
    constructor(token) {
      if (token !== stageToken) throw new TypeError('Stage is created by the player. Use a display object\'s stage property.');
      super(); stages.add(this);
    }
  }
  Stage.__nfIs = value => stages.has(value);
  Stage.__nfInitialize = receiver => {
    if (!stages.has(receiver)) throw new TypeError('Stage cannot be constructed or subclassed by project code.');
  };
  function argumentError(message) { const error = new RangeError(message); error.name = 'ArgumentError'; return error; }
  function securityError(message) { const error = new Error(message); error.name = 'SecurityError'; return error; }
  class StageScaleMode { constructor() { throw new TypeError('StageScaleMode supplies EXACT_FIT, NO_BORDER, NO_SCALE and SHOW_ALL constants.'); } }
  Object.assign(StageScaleMode, { EXACT_FIT: 'exactFit', NO_BORDER: 'noBorder', NO_SCALE: 'noScale', SHOW_ALL: 'showAll' }); Object.freeze(StageScaleMode);
  class StageAlign { constructor() { throw new TypeError('StageAlign supplies stage alignment constants.'); } }
  Object.assign(StageAlign, { BOTTOM: 'B', BOTTOM_LEFT: 'BL', BOTTOM_RIGHT: 'BR', LEFT: 'L', RIGHT: 'R', TOP: 'T', TOP_LEFT: 'TL', TOP_RIGHT: 'TR' }); Object.freeze(StageAlign);
  class StageDisplayState { constructor() { throw new TypeError('StageDisplayState supplies FULL_SCREEN, FULL_SCREEN_INTERACTIVE and NORMAL constants.'); } }
  Object.assign(StageDisplayState, { FULL_SCREEN: 'fullScreen', FULL_SCREEN_INTERACTIVE: 'fullScreenInteractive', NORMAL: 'normal' }); Object.freeze(StageDisplayState);
  class StageQuality { constructor() { throw new TypeError('StageQuality supplies stage rendering quality constants.'); } }
  Object.assign(StageQuality, { LOW: 'low', MEDIUM: 'medium', HIGH: 'high', BEST: 'best', HIGH_8X8: '8x8', HIGH_8X8_LINEAR: '8x8linear', HIGH_16X16: '16x16', HIGH_16X16_LINEAR: '16x16linear' }); Object.freeze(StageQuality);
  class KeyLocation { constructor() { throw new TypeError('KeyLocation supplies STANDARD, LEFT, RIGHT, NUM_PAD and D_PAD constants.'); } }
  Object.assign(KeyLocation, { STANDARD: 0, LEFT: 1, RIGHT: 2, NUM_PAD: 3, D_PAD: 4 }); Object.freeze(KeyLocation);
  class KeyboardType { constructor() { throw new TypeError('KeyboardType supplies ALPHANUMERIC, KEYPAD and NONE constants.'); } }
  Object.assign(KeyboardType, { ALPHANUMERIC: 'alphanumeric', KEYPAD: 'keypad', NONE: 'none' }); Object.freeze(KeyboardType);
  class MouseCursor { constructor() { throw new TypeError('MouseCursor supplies ARROW, AUTO, BUTTON, HAND and IBEAM constants.'); } }
  Object.assign(MouseCursor, { ARROW: 'arrow', AUTO: 'auto', BUTTON: 'button', HAND: 'hand', IBEAM: 'ibeam' }); Object.freeze(MouseCursor);
  const KEYBOARD_CONSTANTS = Object.freeze({ A:65,ALTERNATE:18,AUDIO:0x01000017,B:66,BACK:0x01000016,BACKQUOTE:192,
    BACKSLASH:220,BACKSPACE:8,BLUE:0x01000003,C:67,CAPS_LOCK:20,CHANNEL_DOWN:0x01000005,CHANNEL_UP:0x01000004,
    COMMA:188,COMMAND:15,CONTROL:17,D:68,DELETE:46,DOWN:40,DVR:0x01000019,E:69,END:35,ENTER:13,EQUAL:187,ESCAPE:27,
    EXIT:0x01000015,F:70,F1:112,F10:121,F11:122,F12:123,F13:124,F14:125,F15:126,F2:113,F3:114,F4:115,F5:116,F6:117,
    F7:118,F8:119,F9:120,FAST_FORWARD:0x0100000A,G:71,GREEN:0x01000001,GUIDE:0x01000014,H:72,HELP:0x0100001D,HOME:36,
    I:73,INFO:0x01000013,INPUT:0x0100001B,INSERT:45,J:74,K:75,L:76,LAST:0x01000011,LEFT:37,LEFTBRACKET:219,
    LIVE:0x01000010,M:77,MASTER_SHELL:0x0100001E,MENU:0x01000012,MINUS:189,N:78,NEXT:0x0100000E,NUMBER_0:48,
    NUMBER_1:49,NUMBER_2:50,NUMBER_3:51,NUMBER_4:52,NUMBER_5:53,NUMBER_6:54,NUMBER_7:55,NUMBER_8:56,NUMBER_9:57,
    NUMPAD:21,NUMPAD_0:96,NUMPAD_1:97,NUMPAD_2:98,NUMPAD_3:99,NUMPAD_4:100,NUMPAD_5:101,NUMPAD_6:102,NUMPAD_7:103,
    NUMPAD_8:104,NUMPAD_9:105,NUMPAD_ADD:107,NUMPAD_DECIMAL:110,NUMPAD_DIVIDE:111,NUMPAD_ENTER:108,
    NUMPAD_MULTIPLY:106,NUMPAD_SUBTRACT:109,O:79,P:80,PAGE_DOWN:34,PAGE_UP:33,PAUSE:0x01000008,PERIOD:190,
    PLAY:0x01000007,PLAY_PAUSE:0x01000020,PREVIOUS:0x0100000F,Q:81,QUOTE:222,R:82,RECORD:0x01000006,RED:0x01000000,
    REWIND:0x0100000B,RIGHT:39,RIGHTBRACKET:221,S:83,SEARCH:0x0100001F,SEMICOLON:186,SETUP:0x0100001C,SHIFT:16,
    SKIP_BACKWARD:0x0100000D,SKIP_FORWARD:0x0100000C,SLASH:191,SPACE:32,STOP:0x01000009,SUBTITLE:0x01000018,T:84,
    TAB:9,U:85,UP:38,V:86,VOD:0x0100001A,W:87,X:88,Y:89,YELLOW:0x01000002,Z:90,KEYNAME_BEGIN:'Begin',
    KEYNAME_BREAK:'Break',KEYNAME_CLEARDISPLAY:'ClrDsp',KEYNAME_CLEARLINE:'ClrLn',KEYNAME_DELETE:'Delete',
    KEYNAME_DELETECHAR:'DelChr',KEYNAME_DELETELINE:'DelLn',KEYNAME_DOWNARROW:'Down',KEYNAME_END:'End',
    KEYNAME_EXECUTE:'Exec',KEYNAME_F1:'F1',KEYNAME_F10:'F10',KEYNAME_F11:'F11',KEYNAME_F12:'F12',KEYNAME_F13:'F13',
    KEYNAME_F14:'F14',KEYNAME_F15:'F15',KEYNAME_F16:'F16',KEYNAME_F17:'F17',KEYNAME_F18:'F18',KEYNAME_F19:'F19',
    KEYNAME_F2:'F2',KEYNAME_F20:'F20',KEYNAME_F21:'F21',KEYNAME_F22:'F22',KEYNAME_F23:'F23',KEYNAME_F24:'F24',
    KEYNAME_F25:'F25',KEYNAME_F26:'F26',KEYNAME_F27:'F27',KEYNAME_F28:'F28',KEYNAME_F29:'F29',KEYNAME_F3:'F3',
    KEYNAME_F30:'F30',KEYNAME_F31:'F31',KEYNAME_F32:'F32',KEYNAME_F33:'F33',KEYNAME_F34:'F34',KEYNAME_F35:'F35',
    KEYNAME_F4:'F4',KEYNAME_F5:'F5',KEYNAME_F6:'F6',KEYNAME_F7:'F7',KEYNAME_F8:'F8',KEYNAME_F9:'F9',
    KEYNAME_FIND:'Find',KEYNAME_HELP:'Help',KEYNAME_HOME:'Home',KEYNAME_INSERT:'Insert',KEYNAME_INSERTCHAR:'InsChr',
    KEYNAME_INSERTLINE:'InsLn',KEYNAME_LEFTARROW:'Left',KEYNAME_MENU:'Menu',KEYNAME_MODESWITCH:'ModeSw',
    KEYNAME_NEXT:'Next',KEYNAME_PAGEDOWN:'PgDn',KEYNAME_PAGEUP:'PgUp',KEYNAME_PAUSE:'Pause',
    KEYNAME_PLAYPAUSE:'PlayPause',KEYNAME_PREV:'Prev',KEYNAME_PRINT:'Print',KEYNAME_PRINTSCREEN:'PrntScrn',
    KEYNAME_REDO:'Redo',KEYNAME_RESET:'Reset',KEYNAME_RIGHTARROW:'Right',KEYNAME_SCROLLLOCK:'ScrlLck',
    KEYNAME_SELECT:'Select',KEYNAME_STOP:'Stop',KEYNAME_SYSREQ:'SysReq',KEYNAME_SYSTEM:'Sys',KEYNAME_UNDO:'Undo',
    KEYNAME_UPARROW:'Up',KEYNAME_USER:'User',STRING_BEGIN:'\uf72a',STRING_BREAK:'\uf732',STRING_CLEARDISPLAY:'\uf73a',
    STRING_CLEARLINE:'\uf739',STRING_DELETE:'\uf728',STRING_DELETECHAR:'\uf73e',STRING_DELETELINE:'\uf73c',
    STRING_DOWNARROW:'\uf701',STRING_END:'\uf72b',STRING_EXECUTE:'\uf742',STRING_F1:'\uf704',STRING_F10:'\uf70d',
    STRING_F11:'\uf70e',STRING_F12:'\uf70f',STRING_F13:'\uf710',STRING_F14:'\uf711',STRING_F15:'\uf712',
    STRING_F16:'\uf713',STRING_F17:'\uf714',STRING_F18:'\uf715',STRING_F19:'\uf716',STRING_F2:'\uf705',
    STRING_F20:'\uf717',STRING_F21:'\uf718',STRING_F22:'\uf719',STRING_F23:'\uf71a',STRING_F24:'\uf71b',
    STRING_F25:'\uf71c',STRING_F26:'\uf71d',STRING_F27:'\uf71e',STRING_F28:'\uf71f',STRING_F29:'\uf720',
    STRING_F3:'\uf706',STRING_F30:'\uf721',STRING_F31:'\uf722',STRING_F32:'\uf723',STRING_F33:'\uf724',
    STRING_F34:'\uf725',STRING_F35:'\uf726',STRING_F4:'\uf707',STRING_F5:'\uf708',STRING_F6:'\uf709',
    STRING_F7:'\uf70a',STRING_F8:'\uf70b',STRING_F9:'\uf70c',STRING_FIND:'\uf745',STRING_HELP:'\uf746',
    STRING_HOME:'\uf729',STRING_INSERT:'\uf727',STRING_INSERTCHAR:'\uf73d',STRING_INSERTLINE:'\uf73b',
    STRING_LEFTARROW:'\uf702',STRING_MENU:'\uf735',STRING_MODESWITCH:'\uf747',STRING_NEXT:'\uf740',
    STRING_PAGEDOWN:'\uf72d',STRING_PAGEUP:'\uf72c',STRING_PAUSE:'\uf730',STRING_PREV:'\uf73f',STRING_PRINT:'\uf738',
    STRING_PRINTSCREEN:'\uf72e',STRING_REDO:'\uf744',STRING_RESET:'\uf733',STRING_RIGHTARROW:'\uf703',
    STRING_SCROLLLOCK:'\uf72f',STRING_SELECT:'\uf741',STRING_STOP:'\uf734',STRING_SYSREQ:'\uf731',
    STRING_SYSTEM:'\uf737',STRING_UNDO:'\uf743',STRING_UPARROW:'\uf700',STRING_USER:'\uf736' });
  const CHAR_CODE_STRINGS = Object.freeze(['Up','Down','Left','Right','F1','F2','F3','F4','F5','F6','F7','F8','F9',
    'F10','F11','F12','F13','F14','F15','F16','F17','F18','F19','F20','F21','F22','F23','F24','F25','F26','F27','F28',
    'F29','F30','F31','F32','F33','F34','F35','Insert','Delete','Home','Begin','End','PgUp','PgDn','PrntScrn',
    'ScrlLck','Pause','SysReq','Break','Reset','Stop','Menu','User','Sys','Print','ClrLn','ClrDsp','InsLn','DelLn',
    'InsChr','DelChr','Prev','Next','Select','Exec','Undo','Redo','Find','Help','ModeSw']);
  function createKeyboard(getState = () => ({})) {
    const charCodeStrings = CHAR_CODE_STRINGS.slice();
    class Keyboard { constructor() { throw new TypeError('Keyboard supplies static key constants and state.'); } }
    Object.assign(Keyboard, KEYBOARD_CONSTANTS);
    Object.defineProperties(Keyboard, {
      CharCodeStrings: { enumerable: true, get: () => charCodeStrings },
      capsLock: { enumerable: true, get: () => Boolean(getState().capsLock) },
      numLock: { enumerable: true, get: () => Boolean(getState().numLock) },
      hasVirtualKeyboard: { enumerable: true, get: () => Boolean(getState().hasVirtualKeyboard) },
      physicalKeyboardType: { enumerable: true, get: () => getState().physicalKeyboardType || KeyboardType.ALPHANUMERIC },
      isAccessible: { enumerable: true, value: () => Boolean(getState().accessible) },
    });
    return Object.freeze(Keyboard);
  }
  const VALID_CURSORS = new Set(['auto', 'arrow', 'button', 'hand', 'ibeam']);
  function createMouse(state = { cursor: 'auto', visible: true, supportsCursor: true }, onChange = null) {
    class Mouse { constructor() { throw new TypeError('Mouse supplies static cursor methods and properties.'); } }
    Object.defineProperties(Mouse, {
      cursor: { enumerable: true, get: () => state.cursor,
        set(value) {
          const next = String(value);
          if (!VALID_CURSORS.has(next)) throw argumentError(`Mouse.cursor must be one of auto, arrow, button, hand or ibeam; got '${next}'.`);
          if (state.cursor !== next) { state.cursor = next; onChange?.(); }
        } },
      supportsCursor: { enumerable: true, get: () => Boolean(state.supportsCursor) },
      supportsNativeCursor: { enumerable: true, get: () => false },
      hide: { enumerable: true, value() { if (state.visible) { state.visible = false; onChange?.(); } } },
      show: { enumerable: true, value() { if (!state.visible) { state.visible = true; onChange?.(); } } },
      registerCursor: { enumerable: true, value() { throw new Error('flash.ui.Mouse.registerCursor is not supported in this HTML5 cursor profile.'); } },
      unregisterCursor: { enumerable: true, value() { throw new Error('flash.ui.Mouse.unregisterCursor is not supported in this HTML5 cursor profile.'); } },
    });
    return Object.freeze(Mouse);
  }
  const SCALE_MODES = new Set(['showAll', 'exactFit', 'noBorder', 'noScale']);
  const ALIGN_MODES = Object.freeze({ '': '', T: 'T', B: 'B', L: 'L', R: 'R', TL: 'TL', TR: 'TR', BL: 'BL', BR: 'BR', LT: 'TL', RT: 'TR', LB: 'BL', RB: 'BR' });
  const QUALITY_MODES = Object.freeze({ low: 'LOW', medium: 'MEDIUM', high: 'HIGH', best: 'BEST', '8x8': '8X8', '8x8linear': '8X8LINEAR', '16x16': '16X16', '16x16linear': '16X16LINEAR' });
  const DISPLAY_STATES = new Set(['normal', 'fullScreen', 'fullScreenInteractive']);
  function stageRgbValue(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new TypeError('Stage.color must be a finite integer.');
    return (Math.trunc(number) >>> 0) & 0xffffff;
  }
  function screenMetric(scr, key, fallback) {
    const value = Number(scr?.[key]);
    return Number.isFinite(value) && value > 0 ? (Math.trunc(value) >>> 0) : (Math.trunc(fallback) >>> 0);
  }
  const DefaultKeyboard = createKeyboard();
  const DefaultMouse = createMouse();
  function create(options) {
    if (!eventsAPI || !options?.events) throw new Error('Stage requires the NeoFlash event hub.');
    const { Event, MouseEvent, KeyboardEvent, FocusEvent, setStagePoint } = eventsAPI;
    const hub = options.events;
    const alive = options.alive || (() => true);
    let width = size(options.width, 'stageWidth'), height = size(options.height, 'stageHeight');
    let frameRate = rate(options.frameRate ?? 24), mouseX = 0, mouseY = 0, focus = null;
    let scaleMode = 'showAll', align = '', quality = 'HIGH', displayState = 'normal';
    let stageColor = stageRgbValue(options.color ?? 0xffffff), colorAssigned = false;
    let mouseChildren = true, tabChildren = true, showDefaultContextMenu = true, stageFocusRect = true, invalidated = false;
    const initial = { width, height, frameRate, color: stageColor };
    const keyboardState = { capsLock: Boolean(options.capsLock), numLock: Boolean(options.numLock),
      hasVirtualKeyboard: Boolean(options.hasVirtualKeyboard), physicalKeyboardType: options.physicalKeyboardType || KeyboardType.ALPHANUMERIC,
      accessible: Boolean(options.keyboardAccessible) };
    const mouseStateRecord = { cursor: 'auto', visible: true, supportsCursor: options.supportsCursor ?? true };
    const Keyboard = createKeyboard(() => keyboardState);
    const Mouse = createMouse(mouseStateRecord, () => options.onMouseChanged?.());
    let attached = false, over = null,focusVersion=0,focusDepth=0,nativeFocus=false;
    let captured=null,pressedTarget=null,clickTarget=null,hadPress=false;
    const deliveredPointers=new WeakMap();
    const cleanups = [];
    const stage = new Stage(stageToken);
    const doc = () => options.document || global.document || null;
    function allowsFs(interactive) {
      const explicit = interactive ? (options.allowsFullScreenInteractive ?? options.allowsFullScreen) : options.allowsFullScreen;
      if (explicit !== undefined) return Boolean(explicit);
      const d = doc();
      return Boolean((options.requestFullscreen || options.viewport?.requestFullscreen) && (d ? d.fullscreenEnabled !== false : Boolean(options.requestFullscreen)));
    }
    Object.defineProperties(stage, {
      stageWidth: { enumerable: true, get: () => width }, stageHeight: { enumerable: true, get: () => height },
      frameRate: { enumerable: true, get: () => options.getFrameRate ? options.getFrameRate() : frameRate,
        set(value) { const next = rate(value); if (options.setFrameRate) options.setFrameRate(next); frameRate = next; } },
      scaleMode: { enumerable: true, get: () => scaleMode,
        set(value) {
          const next = String(value);
          if (!SCALE_MODES.has(next)) throw argumentError(`Stage.scaleMode must be showAll, exactFit, noBorder or noScale; got '${next}'.`);
          if (scaleMode !== next) { scaleMode = next; options.onScaleModeChanged?.(scaleMode); }
        } },
      align: { enumerable: true, get: () => align,
        set(value) {
          const key = String(value).toUpperCase();
          if (!Object.hasOwn(ALIGN_MODES, key)) throw argumentError(`Stage.align is invalid: '${value}'.`);
          const next = ALIGN_MODES[key];
          if (align !== next) { align = next; options.onAlignChanged?.(align); }
        } },
      quality: { enumerable: true, get: () => quality,
        set(value) {
          const key = String(value).toLowerCase();
          if (!Object.hasOwn(QUALITY_MODES, key)) throw argumentError(`Stage.quality is invalid: '${value}'.`);
          const next = QUALITY_MODES[key];
          if (quality !== next) { quality = next; options.onQualityChanged?.(quality); }
        } },
      displayState: { enumerable: true, get: () => displayState,
        set(value) {
          const next = String(value);
          if (!DISPLAY_STATES.has(next)) throw argumentError(`Stage.displayState must be normal, fullScreen or fullScreenInteractive; got '${next}'.`);
          if (next === 'normal') {
            if (displayState !== 'normal') {
              displayState = 'normal';
              if (options.exitFullscreen) options.exitFullscreen();
              else if (doc()?.fullscreenElement) doc().exitFullscreen?.();
            }
            return;
          }
          if (!allowsFs(next === 'fullScreenInteractive')) {
            throw securityError('Stage.displayState fullscreen requires browser Fullscreen API permission and an active user gesture.');
          }
          if (options.requestFullscreen) {
            if (options.requestFullscreen(next) === false) throw securityError('Stage.displayState fullscreen request was denied.');
          } else {
            const activation = global.navigator?.userActivation;
            if (activation && !activation.isActive) throw securityError('Stage.displayState fullscreen requires an active user gesture.');
            options.viewport.requestFullscreen();
          }
          displayState = next;
        } },
      color: { enumerable: true, get: () => stageColor,
        set(value) { const next = stageRgbValue(value); stageColor = next; colorAssigned = true; options.onColorChanged?.(next); } },
      fullScreenWidth: { enumerable: true, get: () => screenMetric(options.screen || global.screen, 'width', width) },
      fullScreenHeight: { enumerable: true, get: () => screenMetric(options.screen || global.screen, 'height', height) },
      allowsFullScreen: { enumerable: true, get: () => allowsFs(false) },
      allowsFullScreenInteractive: { enumerable: true, get: () => allowsFs(true) },
      contentsScaleFactor: { enumerable: true, get: () => {
        const dpr = Number(options.contentsScaleFactor ?? options.devicePixelRatio ?? global.devicePixelRatio ?? 1);
        return Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
      } },
      numChildren: { enumerable: true, get: () => options.numChildren ? Math.max(0, Number(options.numChildren()) | 0) : 1 },
      mouseChildren: { enumerable: true, get: () => mouseChildren,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.mouseChildren requires a Boolean.'); mouseChildren = value; } },
      tabChildren: { enumerable: true, get: () => tabChildren,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.tabChildren requires a Boolean.'); tabChildren = value; } },
      showDefaultContextMenu: { enumerable: true, get: () => showDefaultContextMenu,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.showDefaultContextMenu requires a Boolean.'); showDefaultContextMenu = value; } },
      stageFocusRect: { enumerable: true, get: () => stageFocusRect,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.stageFocusRect requires a Boolean.'); stageFocusRect = value; } },
      mouseX: { enumerable: true, get: () => mouseX }, mouseY: { enumerable: true, get: () => mouseY },
      focus: { enumerable: true, get() { if (focus && !alive(focus)) changeFocus(null); return focus; },
        set(value) {changeFocus(value);} },
      stage: { enumerable: true, get: () => stage }, parent: { enumerable: true, get: () => null },
      root: { enumerable: true, get: () => stage },
      invalidate: { value() { if (!invalidated) { invalidated = true; options.onInvalidate?.(); } } },
      toString: { value: () => '[NeoFlash Stage]' },
    });
    Object.assign(stage, hub.methods(stage));
    // Stage is a sealed native surface; unsupported scale/fullscreen properties must
    // not appear to work merely because JavaScript accepts arbitrary new fields.
    Object.freeze(stage);
    function changeFocus(value,reason='programmatic',domEvent={}) {
      if(value!==null&&(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('Stage.focus requires a display object or null.');
      if(value!==null&&!alive(value))throw new Error('Stage.focus requires an active display object or null.');
      if(value!==null&&options.canFocus&&!options.canFocus(value)){
        if(reason==='mouse')value=null;
        else throw new Error('This display object is not currently focusable.');
      }
      if(value===focus)return true;
      if(focusDepth>=64)throw new RangeError('Recursive focus changes exceeded 64 transitions.');
      focusDepth++;
      try{
        const previous=focus;
        if((reason==='mouse'||reason==='key')&&previous&&alive(previous)){
          const event=new FocusEvent(reason==='mouse'?FocusEvent.MOUSE_FOCUS_CHANGE:FocusEvent.KEY_FOCUS_CHANGE,true,true,value,domEvent.shiftKey,reason==='key'?9:0);
          if(!hub.dispatch(previous,event)){domEvent.preventDefault?.();return false;}
          if(focus!==previous)return false;
        }
        focus=value;nativeFocus=false;const version=++focusVersion;
        if(previous&&alive(previous))hub.dispatch(previous,new FocusEvent(FocusEvent.FOCUS_OUT,true,false,value));
        if(version!==focusVersion)return true;
        const handled=options.onFocusChanged?.(value,previous)===true;
        if(version!==focusVersion)return true;
        nativeFocus=handled;
        if(value&&!handled)options.canvas?.focus?.({preventScroll:true});
        if(value&&alive(value))hub.dispatch(value,new FocusEvent(FocusEvent.FOCUS_IN,true,false,previous));
        return true;
      }finally{focusDepth--;}
    }
    function size(value, name) {
      value = Number(value);
      if (!Number.isFinite(value) || value <= 0) throw new TypeError(`${name} must be positive and finite.`);
      return value;
    }
    function rate(value) {
      value = Number(value);
      if (!Number.isFinite(value) || value <= 0 || value > 1000) throw new RangeError('Stage.frameRate must be in (0, 1000].');
      return value;
    }
    function enabled() { return !options.isInputEnabled || options.isInputEnabled(); }
    function targetAt(domEvent, explicitTarget) {
      if (!mouseChildren) return stage;
      const supplied=explicitTarget&&explicitTarget!==stage&&options.resolveInput?options.resolveInput(explicitTarget):explicitTarget;
      const target = explicitTarget ? supplied||stage : options.hitTest?.(domEvent)||stage;
      return target === stage || alive(target) ? target : stage;
    }
    function updatePoint(domEvent) {
      const point = options.coordinates?.(domEvent);
      if (point && point.length === 2 && point.every(Number.isFinite)) [mouseX, mouseY] = point;
    }
    function mouse(type, target, domEvent, related = null) {
      const point = target === stage ? [mouseX, mouseY] : options.localPoint?.(target, mouseX, mouseY) || [mouseX, mouseY];
      const event = new MouseEvent(type, ![MouseEvent.ROLL_OVER,MouseEvent.ROLL_OUT].includes(type), true, point[0], point[1], related,
        Boolean(domEvent.ctrlKey || domEvent.metaKey), domEvent.altKey, domEvent.shiftKey,
        Boolean((domEvent.buttons || 0) & 1), type === MouseEvent.MOUSE_WHEEL ? -Math.sign(domEvent.deltaY || 0) : 0,
        domEvent.metaKey, domEvent.ctrlKey, domEvent.detail || 0);
      return setStagePoint(event, mouseX, mouseY);
    }
    function deliver(target, event, domEvent) {
      const result = hub.dispatch(target, event);
      if (!result && domEvent.cancelable !== false) domEvent.preventDefault?.();
      return result;
    }
    function releaseCapture(){const id=captured;captured=null;if(id!==null&&options.canvas?.hasPointerCapture?.(id))options.canvas.releasePointerCapture(id);}
    function cancelPointer(){pressedTarget=null;clickTarget=null;hadPress=false;releaseCapture();over=null;return options.cancelPointer?.();}
    function chain(target){const result=[];while(target&&target!==stage&&alive(target)&&result.length<65){result.push(target);target=options.parentOf?options.parentOf(target):target.parent;}return result;}
    function hover(target,domEvent){
      if(target===over)return;const previous=over;over=target;
      if(previous&&previous!==stage&&alive(previous))deliver(previous,mouse(MouseEvent.MOUSE_OUT,previous,domEvent,target),domEvent);
      const old=chain(previous),next=chain(target);
      for(const item of old)if(!next.includes(item)&&alive(item))deliver(item,mouse(MouseEvent.ROLL_OUT,item,domEvent,target),domEvent);
      for(const item of [...next].reverse())if(!old.includes(item)&&alive(item))deliver(item,mouse(MouseEvent.ROLL_OVER,item,domEvent,previous),domEvent);
      if(target!==stage&&alive(target))deliver(target,mouse(MouseEvent.MOUSE_OVER,target,domEvent,previous),domEvent);
    }
    function handlePointer(type, domEvent = {}, explicitTarget = null, physicalPick = false) {
      if(['pointercancel','lostpointercapture','cancel'].includes(type)){if((type!=='lostpointercapture'||captured!==null)&&cancelPointer())options.afterInput?.();return true;}
      if(!enabled()||domEvent.pointerType==='touch')return true;
      if(domEvent.isPrimary===false&&/^(?:pointerdown|pointerup|pointermove)$/.test(type))return true;
      const mapped={pointerdown:'mouseDown',pointerup:'mouseUp',pointermove:'mouseMove',pointerleave:'mouseMove',mousedown:'mouseDown',mouseup:'mouseUp',mousemove:'mouseMove',dblclick:'doubleClick',wheel:'mouseWheel'};
      type=mapped[type]||type;
      if(!['mouseDown','mouseUp','mouseMove','mouseWheel','click','doubleClick'].includes(type))throw new Error(`Unsupported native mouse event '${type}'.`);
      // Native editor events are forwarded directly, then bubble to the outer
      // pointer listener. One physical Event must enter the host only once.
      if(domEvent&&typeof domEvent==='object'&&typeof domEvent.type==='string'){let seen=deliveredPointers.get(domEvent);if(seen?.has(type))return true;if(!seen){seen=new Set();deliveredPointers.set(domEvent,seen);}seen.add(type);}
      updatePoint(domEvent);options.beforePointer?.(type,domEvent);
      const target=targetAt(domEvent,explicitTarget),primary=domEvent.button===undefined||domEvent.button===0;
      if(type==='click'&&(!explicitTarget||physicalPick)&&hadPress&&clickTarget!==target)return false;
      if(type===MouseEvent.MOUSE_DOWN&&primary){
        pressedTarget=target;clickTarget=null;hadPress=true;
        if(domEvent.target===options.canvas&&Number.isInteger(domEvent.pointerId)&&options.canvas?.setPointerCapture){options.canvas.setPointerCapture(domEvent.pointerId);captured=domEvent.pointerId;}
        const changed=changeFocus(target===stage?null:target,'mouse',domEvent);
        if(changed&&nativeFocus&&domEvent.target===options.canvas)domEvent.preventDefault?.();
      }
      if(type===MouseEvent.MOUSE_MOVE||type===MouseEvent.MOUSE_DOWN)hover(target,domEvent);
      if(target!==stage&&!alive(target)){over=null;options.afterInput?.();return true;}
      if(type===MouseEvent.MOUSE_UP&&primary){clickTarget=pressedTarget===target?target:null;pressedTarget=null;releaseCapture();}
      const result=deliver(target,mouse(type,target,domEvent),domEvent);
      if(target===stage||alive(target))options.onLegacyInput?.(type,target,domEvent);
      options.onPointerTarget?.(target,type);options.afterInput?.();return result;
    }
    function handleKey(type, domEvent = {}) {
      if (!enabled() || domEvent.isComposing) return true;
      const nativeTarget = domEvent.target;
      if (nativeTarget && nativeTarget !== options.canvas && !options.ownsTextInput?.(nativeTarget) &&
        (nativeTarget.isContentEditable || /^(?:INPUT|TEXTAREA|SELECT)$/.test(nativeTarget.tagName || ''))) return true;
      if (type === 'keydown') type = KeyboardEvent.KEY_DOWN;
      if (type === 'keyup') type = KeyboardEvent.KEY_UP;
      if (![KeyboardEvent.KEY_DOWN, KeyboardEvent.KEY_UP].includes(type)) throw new Error(`Unsupported native keyboard event '${type}'.`);
      if (typeof domEvent.getModifierState === 'function') {
        keyboardState.capsLock = Boolean(domEvent.getModifierState('CapsLock'));
        keyboardState.numLock = Boolean(domEvent.getModifierState('NumLock'));
      }
      const target = stage.focus || stage;
      const text = typeof domEvent.key === 'string' ? domEvent.key : '';
      const charCode = [...text].length === 1 ? text.codePointAt(0) : 0;
      const event = new KeyboardEvent(type, true, true, charCode, keyCode(domEvent), domEvent.location || 0,
        Boolean(domEvent.ctrlKey || domEvent.metaKey), domEvent.altKey, domEvent.shiftKey, domEvent.ctrlKey, domEvent.metaKey);
      const result = deliver(target, event, domEvent);
      if (target === stage || alive(target)) options.onLegacyInput?.(type, target, domEvent);
      options.afterInput?.();
      return result;
    }
    function keyCode(event) {
      if (Number.isInteger(event.keyCode) && event.keyCode > 0) return event.keyCode;
      const code = event.code || '';
      if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
      if (/^Digit[0-9]$/.test(code)) return code.charCodeAt(5);
      if (/^Numpad[0-9]$/.test(code)) return 96 + Number(code.slice(6));
      if (/^F(?:[1-9]|1[0-5])$/.test(code)) return 111 + Number(code.slice(1));
      return ({ ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Enter: 13, Space: 32,
        Escape: 27, Tab: 9, Backspace: 8, Delete: 46, Insert: 45, Home: 36, End: 35, PageUp: 33, PageDown: 34,
        CapsLock: 20, ShiftLeft: 16, ShiftRight: 16, ControlLeft: 17, ControlRight: 17, AltLeft: 18, AltRight: 18,
        MetaLeft: 15, MetaRight: 15, NumpadMultiply: 106, NumpadAdd: 107, NumpadEnter: 108, NumpadSubtract: 109,
        NumpadDecimal: 110, NumpadDivide: 111, Semicolon: 186, Equal: 187, Comma: 188, Minus: 189, Period: 190,
        Slash: 191, Backquote: 192, BracketLeft: 219, Backslash: 220, BracketRight: 221, Quote: 222 })[code] || 0;
    }
    function resize(nextWidth, nextHeight) {
      nextWidth = size(nextWidth, 'stageWidth'); nextHeight = size(nextHeight, 'stageHeight');
      if (nextWidth === width && nextHeight === height) return false;
      width = nextWidth; height = nextHeight;
      hub.dispatch(stage, new Event(Event.RESIZE)); options.afterInput?.();
      return true;
    }
    function listen(target, type, fn, settings) {
      if (!target?.addEventListener) return;
      const guarded = event => {
        try { fn(event); } catch (error) { if (options.onError) options.onError(error); else throw error; }
      };
      target.addEventListener(type, guarded, settings);
      cleanups.push(() => target.removeEventListener(type, guarded, settings));
    }
    function attach() {
      if (attached) return;
      attached = true;
      for (const type of ['pointerdown','pointerup','pointermove','pointercancel','lostpointercapture','pointerleave']) listen(options.canvas,type,event=>handlePointer(type,event));
      const outside=options.pointerTarget||global;
      for(const type of ['pointermove','pointerup','pointercancel'])listen(outside,type,event=>{if(event.target!==options.canvas&&(captured!==null||pressedTarget!==null||options.hasDrag?.()))handlePointer(type,event);});
      listen(outside,'blur',()=>{if(cancelPointer())options.afterInput?.();});
      listen(options.canvas, 'wheel', event => handlePointer('wheel', event), { passive: false });
      listen(options.canvas, 'contextmenu', event => { if (!showDefaultContextMenu) event.preventDefault?.(); });
      if (doc()) listen(doc(), 'fullscreenchange', () => { if (!doc()?.fullscreenElement) displayState = 'normal'; });
      // Existing runtime click owns authored Click dispatch; opt in only for standalone use.
      if (options.attachClick) listen(options.canvas, 'click', event => handlePointer('click', event));
      for (const type of ['keydown', 'keyup']) listen(options.keyboardTarget || options.canvas, type, event => handleKey(type, event));
      if (options.getSize) listen(options.resizeTarget || global, 'resize', () => {
        const dimensions = options.getSize(); resize(dimensions[0], dimensions[1]);
      });
    }
    function flushRender(targets = []) {
      if (!invalidated) return false;
      invalidated = false;
      hub.broadcast(Event.RENDER || 'render', [stage, ...targets]);
      return true;
    }
    function resetState() {
      scaleMode = 'showAll'; align = ''; quality = 'HIGH'; displayState = 'normal';
      stageColor = initial.color; colorAssigned = false;
      mouseChildren = true; tabChildren = true; showDefaultContextMenu = true; stageFocusRect = true; invalidated = false;
      keyboardState.capsLock = Boolean(options.capsLock); keyboardState.numLock = Boolean(options.numLock);
      mouseStateRecord.cursor = 'auto'; mouseStateRecord.visible = true;
      options.onMouseChanged?.();
    }
    function dispose() { cancelPointer();for (const cleanup of cleanups.splice(0)) cleanup(); attached = false; hub.release(stage); focus = null; nativeFocus=false; over = null; resetState(); }
    function reset() {
      cancelPointer();
      focus = null; nativeFocus=false; focusVersion++; over = null; mouseX = 0; mouseY = 0;
      width = initial.width; height = initial.height;
      if (options.setFrameRate) options.setFrameRate(initial.frameRate);
      frameRate = initial.frameRate;
      resetState();
      hub.release(stage);
    }
    return Object.freeze({ stage, Keyboard, Mouse, mouseState: () => ({ cursor: mouseStateRecord.cursor, visible: mouseStateRecord.visible }),
      colorAssigned: () => colorAssigned, flushRender, handlePointer, handleKey, cancelPointer, changeFocus, resize, attach, dispose, reset,
      tick(targets = []) { hub.broadcast(Event.ENTER_FRAME, [stage, ...targets]); flushRender(targets); },
    });
  }
  const api = Object.freeze({ create, Stage, StageScaleMode, StageAlign, StageDisplayState, StageQuality,
    Keyboard: DefaultKeyboard, KeyLocation, KeyboardType, Mouse: DefaultMouse, MouseCursor, createKeyboard, createMouse });
  global.NeoFlashStage = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
