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
  function create(options) {
    if (!eventsAPI || !options?.events) throw new Error('Stage requires the NeoFlash event hub.');
    const { Event, MouseEvent, KeyboardEvent, FocusEvent, setStagePoint } = eventsAPI;
    const hub = options.events;
    const alive = options.alive || (() => true);
    let width = size(options.width, 'stageWidth'), height = size(options.height, 'stageHeight');
    let frameRate = rate(options.frameRate ?? 24), mouseX = 0, mouseY = 0, focus = null;
    const initial = { width, height, frameRate };
    let attached = false, over = null,focusVersion=0,focusDepth=0,nativeFocus=false;
    let captured=null,pressedTarget=null,clickTarget=null,hadPress=false;
    const deliveredPointers=new WeakMap();
    const cleanups = [];
    const stage = new Stage(stageToken);
    Object.defineProperties(stage, {
      stageWidth: { enumerable: true, get: () => width }, stageHeight: { enumerable: true, get: () => height },
      frameRate: { enumerable: true, get: () => options.getFrameRate ? options.getFrameRate() : frameRate,
        set(value) { const next = rate(value); if (options.setFrameRate) options.setFrameRate(next); frameRate = next; } },
      mouseX: { enumerable: true, get: () => mouseX }, mouseY: { enumerable: true, get: () => mouseY },
      focus: { enumerable: true, get() { if (focus && !alive(focus)) changeFocus(null); return focus; },
        set(value) {changeFocus(value);} },
      stage: { enumerable: true, get: () => stage }, parent: { enumerable: true, get: () => null },
      root: { enumerable: true, get: () => stage },
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
    function cancelPointer(){pressedTarget=null;clickTarget=null;hadPress=false;releaseCapture();over=null;options.cancelPointer?.();}
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
      if(['pointercancel','lostpointercapture','cancel'].includes(type)){if(type!=='lostpointercapture'||captured!==null)cancelPointer();return true;}
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
      if (/^F(?:[1-9]|1[0-5])$/.test(code)) return 111 + Number(code.slice(1));
      return ({ ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Enter: 13, Space: 32,
        Escape: 27, Tab: 9, Backspace: 8, Delete: 46, Home: 36, End: 35, PageUp: 33, PageDown: 34,
        ShiftLeft: 16, ShiftRight: 16, ControlLeft: 17, ControlRight: 17, AltLeft: 18, AltRight: 18 })[code] || 0;
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
      listen(outside,'blur',cancelPointer);
      listen(options.canvas, 'wheel', event => handlePointer('wheel', event), { passive: false });
      // Existing runtime click owns authored Click dispatch; opt in only for standalone use.
      if (options.attachClick) listen(options.canvas, 'click', event => handlePointer('click', event));
      for (const type of ['keydown', 'keyup']) listen(options.keyboardTarget || options.canvas, type, event => handleKey(type, event));
      if (options.getSize) listen(options.resizeTarget || global, 'resize', () => {
        const dimensions = options.getSize(); resize(dimensions[0], dimensions[1]);
      });
    }
    function dispose() { cancelPointer();for (const cleanup of cleanups.splice(0)) cleanup(); attached = false; hub.release(stage); focus = null; nativeFocus=false; over = null; }
    function reset() {
      cancelPointer();
      focus = null; nativeFocus=false; focusVersion++; over = null; mouseX = 0; mouseY = 0;
      width = initial.width; height = initial.height;
      if (options.setFrameRate) options.setFrameRate(initial.frameRate);
      frameRate = initial.frameRate;
      hub.release(stage);
    }
    return Object.freeze({ stage, handlePointer, handleKey, cancelPointer, changeFocus, resize, attach, dispose, reset,
      tick(targets) { hub.broadcast(Event.ENTER_FRAME, [stage, ...targets]); },
    });
  }
  const api = Object.freeze({ create, Stage });
  global.NeoFlashStage = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
