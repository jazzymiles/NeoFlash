/* NeoFlash HTML5 host. Timeline evaluation and display-list construction run in shared Rust/WASM. */
(function (global) {
  'use strict';

  function localPoint(matrix, x, y) {
    const [a, b, c, d, tx, ty] = matrix;
    const determinant = a * d - b * c;
    if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-12) return null;
    return [(d * (x - tx) - c * (y - ty)) / determinant, (-b * (x - tx) + a * (y - ty)) / determinant];
  }
  function hitShape(kind, x, y, measureText, testPath) {
    if (kind.type === 'rectangle') return x >= 0 && y >= 0 && x <= kind.width && y <= kind.height;
    if (kind.type === 'ellipse') {
      if (kind.width <= 0 || kind.height <= 0) return false;
      return Math.pow((x - kind.width / 2) / (kind.width / 2), 2) + Math.pow((y - kind.height / 2) / (kind.height / 2), 2) <= 1;
    }
    if (kind.type === 'text') {
      const lines = kind.text.split('\n');
      return x >= 0 && y >= 0 && y <= lines.length * kind.font_size * 1.2 &&
        x <= Math.max(0, ...lines.map(line => measureText(line, kind)));
    }
    if (kind.type === 'path') return testPath(kind, x, y);
    return false;
  }
  function oneBasedFrame(value, total, resolveLabel) {
    if (typeof value === 'string') {
      const label = resolveLabel(value);
      if (label >= 0) return label;
      if (!/^\d+$/.test(value)) throw new Error(`Frame label '${value}' does not exist.`);
      value = Number(value);
    }
    if (!Number.isInteger(value) || value < 1 || value > total) throw new Error(`Frame must be between 1 and ${total}.`);
    return value - 1;
  }
  // AS2 label lookup is separate from the strict JavaScript/AS3 bridge.
  function as2Frame(value, total, resolveLabel) {
    // AVM1 parses numeric strings before consulting labels. Keep the numeric
    // compatibility boundary explicit instead of guessing clamping/wrapping.
    if (typeof value === 'string' && /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$|^[+-]?(?:Infinity|NaN)$/.test(value)) value = Number(value);
    if (typeof value === 'number') {
      if (!Number.isInteger(value) || value < 1 || value > total) {
        throw new Error(`AS2 numeric goto currently supports integer frames 1 through ${total}; legacy clamping, wrapping and other numeric coercions are not supported.`);
      }
      return value - 1;
    }
    if (typeof value === 'string') {
      const frame = resolveLabel(value);
      return frame >= 0 ? frame : null;
    }
    throw new Error('AS2 goto requires a frame label or integer frame; other argument coercions are not supported.');
  }
  function createAs2Methods() {
    const variants = new WeakMap();
    function aliasError() {
      return new TypeError('AS2 goto method aliases need receiver semantics not supported yet. Call gotoAndPlay/gotoAndStop directly on the target timeline.');
    }
    return {
      register(method, receiver, name, implementation) { variants.set(method, { receiver, name, implementation }); },
      method(receiver, name) {
        // Resolve the getter before arguments, then check callability after
        // argument evaluation, preserving ordinary call evaluation order.
        const method = receiver[name];
        return (...args) => {
          if (typeof method !== 'function') {
            throw new TypeError(`AS2 timeline function '${name}' is missing or is not callable. Define it on this timeline before calling it.`);
          }
          const variant = variants.get(method);
          if (variant && (variant.receiver !== receiver || variant.name !== name)) throw aliasError();
          return Reflect.apply(variant ? variant.implementation : method, receiver, args);
        };
      },
      value(method, receiver) {
        return (...args) => {
          if (variants.has(method)) throw aliasError();
          if (typeof method !== 'function') throw new TypeError('AS2 function value is not callable. Assign a function before calling it.');
          return Reflect.apply(method, receiver, args);
        };
      },
    };
  }
  function color(rgba) { return `rgba(${rgba[0]},${rgba[1]},${rgba[2]},${rgba[3] / 255})`; }
  function sourceKey(script) { return script.language + '\u0000' + script.source; }
  const as2Thrown = Symbol.for('neoflash.as2.thrown.v1');
  function unwrapScriptThrow(value) {
    // Only compiler transport envelopes have these own data properties. Do not
    // invoke arbitrary accessors while trying to report an author's exception.
    try {
      if (value !== null && (typeof value === 'object' || typeof value === 'function')) {
        const brand = Object.getOwnPropertyDescriptor(value, as2Thrown);
        const payload = Object.getOwnPropertyDescriptor(value, 'value');
        if (brand && brand.value === true && payload && Object.hasOwn(payload, 'value')) return payload.value;
      }
    } catch (_) { /* A revoked or hostile Proxy is still the original failure. */ }
    return value;
  }
  function describeThrown(value, includeStack = false) {
    if (value !== null && value !== undefined) {
      if (includeStack) {
        try { const stack = value.stack; if (typeof stack === 'string' && stack) return stack; } catch (_) {}
      }
      try { const message = value.message; if (typeof message === 'string') return message; } catch (_) {}
    }
    try { return String(value); } catch (_) { return '<unprintable thrown value>'; }
  }
  const helpers = { localPoint, hitShape, oneBasedFrame, as2Frame, createAs2Methods, sourceKey, describeThrown, unwrapScriptThrow };
  if (typeof module !== 'undefined' && module.exports) module.exports = helpers;
  if (typeof document === 'undefined') return;

  const errorElement = document.getElementById('error');
  let failed = false;
  let stopAsyncOnFailure = () => {};
  function fail(error) {
    failed = true;
    try { stopAsyncOnFailure(); } catch (cleanupError) { console.error(cleanupError); }
    document.getElementById('loading').style.display = 'none';
    errorElement.style.display = 'block';
    errorElement.textContent = 'NeoFlash could not run this project.\n\n' + describeThrown(error, true);
    console.error(error);
  }

  async function boot() {
    const bundle = global.NeoFlashBundle;
    if (!bundle || typeof NeoFlashWasm !== 'function') throw new Error('The HTML5 runtime is incomplete. Re-export this project from NeoFlash.');
    const bytes = Uint8Array.from(atob(bundle.wasm), character => character.charCodeAt(0));
    await NeoFlashWasm({ module_or_path: bytes });
    const project = bundle.project;
    const player = new NeoFlashWasm.WebPlayer(JSON.stringify(project));
    const canvas = document.getElementById('stage');
    const context = canvas.getContext('2d');
    if (!context) throw new Error('This browser does not provide Canvas 2D.');
    if (!global.NeoFlashPaths) throw new Error('The vector path runtime is missing. Re-export this project.');
    const paths = new global.NeoFlashPaths.PathCache();
    const status = document.getElementById('status');
    const toggle = document.getElementById('toggle');
    const traceElement = document.getElementById('trace');
    const viewport = document.getElementById('viewport');
    const semanticLayer = new global.NeoFlashAccessibility.SemanticLayer(document.getElementById('semantic-layer'), node => {
      if (failed) return;
      try {
        if (player.activate_accessible(JSON.stringify(node.instance_path), node.lifetime_id)) {
          const target=proxyFor(node.instance_path);
          if(stageHost){stageHost.stage.focus=target;stageHost.handlePointer('click',{},target);}
          dispatchScripts(target); render();
        }
      } catch (error) { fail(error); }
    });
    const preview = new global.NeoFlashPreview.Preview(viewport, document.getElementById('stage-space'), document.getElementById('stage-surface'), canvas, project, () => render());
    const functions = new Map();
    const proxies = new Map();
    let scene = [];
    let lastTime = null;
    let accumulated = 0;
    let lastTrace = '';
    const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame', '__nfAs2Method', '__nfAs2Value', '__nfProgram', '__nfDisplay', '__nfLegacy'];
    for (const script of bundle.scripts) {
      try {
        functions.set(sourceKey(script), {
          fn: new Function(...bindings, '"use strict";\nreturn (function () {\n' + script.code + '\n}).call(this);\n//# sourceURL=neoflash-user-script.js'),
          location: script.location,
        });
      } catch (error) { throw new Error(`Script syntax error in ${script.location}: ${describeThrown(error)}`, { cause: error }); }
    }
    for (const warning of bundle.warnings) console.warn('NeoFlash:', warning);

    const propertyMap = {
      x: 'x', y: 'y', rotation: 'rotation', alpha: 'alpha', scaleX: 'scale_x', scaleY: 'scale_y',
      _x: 'x', _y: 'y', _rotation: 'rotation', _alpha: 'alpha', _xscale: 'scale_x', _yscale: 'scale_y',
    };
    const graphMode=typeof player.runtime_snapshot_json==='function';
    const references = new WeakMap();
    const attachedClasses = new Map();
    let display = null, eventHub = null, stageHost = null, legacyHost = null, legacyGlobals = null, timerHost = null, binaryHost = null, program = null;
    let timerRenderGeneration=0;
    let legacyCursorVisible = true, pointerOverContent = false;
    let runtimeFrameRate = typeof project === 'undefined' ? 24 : project.fps;
    function updateCursor() {
      if(typeof canvas!=='undefined')canvas.style.cursor=!legacyCursorVisible?'none':typeof preview!=='undefined'&&preview.panMode?'grab':pointerOverContent?'pointer':'default';
    }
    function inputFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Input callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else throw failure;
    }
    function stopTimers() {timerRenderGeneration++;timerHost?.reset();}
    function afterTimerEvent() {dispatchScripts();if(typeof context!=='undefined')render();}
    function timerFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Timer callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {stopTimers();throw failure;}
    }
    function requestTimerRender() {
      const generation=timerRenderGeneration;
      global.requestAnimationFrame(()=>{
        if(generation!==timerRenderGeneration||(typeof failed!=='undefined'&&failed))return;
        try {afterTimerEvent();}catch(error){timerFailure(error);}
      });
    }
    function initializeFamily() {
      if (!global.NeoFlashDisplay || typeof player.display_nodes_json !== 'function') return;
      display=global.NeoFlashDisplay.create({player,proxyFor,proxyForRef,onMutation:deliverDisplayEvents,referenceOf:target=>references.get(target),getStage:()=>stageHost?.stage||null,
        measureText:(line,kind)=>{if(typeof context==='undefined')throw new Error('Text bounds require the Canvas font measurement service.');context.font=font(kind);return context.measureText(line).width;},
        pathBounds:(commands,matrix)=>global.NeoFlashAccessibility.pathBounds(commands,matrix)});
      if(global.NeoFlashEvents&&global.NeoFlashStage) {
        const alive=target=>target===stageHost?.stage||display.alive(target);
        const local=(target,x,y)=>{const p=target===stageHost?.stage?{x,y}:display.convert(target,{x,y},false);return[p.x,p.y];};
        eventHub=global.NeoFlashEvents.create({alive,parentOf:target=>target===stageHost?.stage?null:target.parent,
          localPoint:local,globalPoint:(target,x,y)=>{const p=target===stageHost?.stage?{x,y}:display.convert(target,{x,y},true);return[p.x,p.y];}});
        stageHost=global.NeoFlashStage.create({events:eventHub,width:typeof project==='undefined'?800:project.width,height:typeof project==='undefined'?600:project.height,
          frameRate:runtimeFrameRate,getFrameRate:()=>runtimeFrameRate,setFrameRate:value=>{runtimeFrameRate=value;},alive:target=>alive(target)&&(target===stageHost?.stage||display.descriptor(target).on_stage!==false),localPoint:local,
          canvas:typeof canvas==='undefined'?null:canvas,keyboardTarget:typeof viewport==='undefined'?null:viewport,
          hitTest:event=>{const hit=hitAt(event);return hit?interactiveTarget(hit):null;},
          coordinates:event=>{if(typeof canvas==='undefined')return null;const r=canvas.getBoundingClientRect();return[(event.clientX-r.left)*project.width/r.width,(event.clientY-r.top)*project.height/r.height];},
          isInputEnabled:()=>typeof failed==='undefined'||(!failed&&!preview.panMode&&!preview.suppressClick),
          onLegacyInput:(type,target,event)=>{if(type==='mouseDown'&&(event.button===undefined||event.button===0))display.press(target);},
          afterInput:()=>{dispatchScripts();if(typeof context!=='undefined')render();},onError:inputFailure});
        if(global.NeoFlashLegacyInput){
          legacyHost=global.NeoFlashLegacyInput.create({stage:stageHost.stage,
            keyTarget:typeof global.addEventListener==='function'?global:null,
            alive:listener=>!references.has(listener)||display.alive(listener),
            onVisibility:visible=>{legacyCursorVisible=visible;updateCursor();},onError:inputFailure});
          legacyGlobals=Object.freeze({Key:legacyHost.Key,Mouse:legacyHost.Mouse,Stage:legacyHost.Stage});
        }
      }
      if(global.NeoFlashTimers&&global.NeoFlashEvents)timerHost=global.NeoFlashTimers.create({events:global.NeoFlashEvents,
        onError:timerFailure,afterEvent:afterTimerEvent,requestRender:requestTimerRender});
      if(global.NeoFlashByteArray)binaryHost=global.NeoFlashByteArray.create();
      initializeProgram();
    }
    function initializeProgram() {
      if(!display)return;
      const builtins={...display.builtins};
      builtins.__nfTrace=trace;
      builtins.__nfAs2Method=as2.method;builtins.__nfAs2Value=as2.value;
      builtins.__nfDisplay=display;builtins.__nfLegacy=legacyGlobals;
      if(global.NeoFlashEvents)for(const name of ['Event','MouseEvent','KeyboardEvent','EventDispatcher','EventPhase'])builtins['flash.events.'+name]=global.NeoFlashEvents[name];
      if(global.NeoFlashStage)builtins['flash.display.Stage']=global.NeoFlashStage.Stage;
      if(timerHost){builtins['flash.utils.Timer']=timerHost.Timer;builtins['flash.events.TimerEvent']=timerHost.TimerEvent;builtins['flash.utils.getTimer']=timerHost.getTimer;}
      if(binaryHost){builtins['flash.utils.ByteArray']=binaryHost.ByteArray;builtins['flash.utils.Endian']=binaryHost.Endian;}
      program=typeof bundle!=='undefined'&&bundle.program ? new Function('__nfBuiltins',bundle.program.code)(builtins) : {
        type:name=>{if(!builtins[name])throw new TypeError(`Unsupported built-in class ${name}.`);return builtins[name];},
        is:(value,name)=>Boolean(builtins[name]?.__nfIs?.(value)),
      };
    }
    function resetProject() {
      stopTimers();binaryHost?.reset();
      proxies.clear(); attachedClasses.clear();display?.clear();eventHub?.clear();stageHost?.reset();legacyHost?.reset();lastTrace = '';
      player.reset();
      initializeProgram();
      dispatchScripts();
    }
    function disposeFamily() {
      timerRenderGeneration++;timerHost?.dispose();binaryHost?.dispose();
      legacyHost?.dispose();stageHost?.dispose();eventHub?.clear();display?.clear();
    }
    function attachNewInstances() {
      if(!display)return;
      display.prune();eventHub?.prune();
      // Linked descendants are initialized before their container/document class,
      // so the constructor can call an authored child's initialized methods.
      const nodes=display.nodes();
      for(const node of nodes.sort((a,b)=>(b.authored_path||b.path).length-(a.authored_path||a.path).length)) {
        if(graphMode&&node.authored_path===null)continue; // dynamic constructors attach at allocation
        const key=graphMode?JSON.stringify(node.object_ref):JSON.stringify(node.path),previous=attachedClasses.get(key);
        const lifetime=graphMode?node.object_ref.generation:node.lifetime_id;
        if(graphMode?!player.object_alive(key):player.lifetime_id(key)!==lifetime)continue;
        if(previous===lifetime)continue;
        attachedClasses.set(key,lifetime);
        const receiver=graphMode?proxyForRef(node.object_ref):proxyFor(node.path);
        if(node.class_name){
          if(!program?.attach)throw new Error(`Linked class ${node.class_name} has no compiled project program.`);
          program.attach(node.class_name,receiver);
          if(program.dialect?.(node.class_name)==='as2')display.refreshLegacyCallbacks(receiver);
        }
      }
      for(const key of attachedClasses.keys())if(graphMode?!player.object_alive(key):!player.lifetime_id(key))attachedClasses.delete(key);
      drainDisplayEvents();
      // Canonical refs are never reused; retire cache entries without retaining every
      // prior authored placement forever. Externally held stale proxies still reject access.
      if(graphMode)for(const key of proxies.keys())if(!player.object_alive(key))proxies.delete(key);
    }
    function deliverDisplayEvents(events) {
      if(!eventHub||!events?.length)return;
      const rootRef=JSON.parse(player.object_ref('[]')),rootKey=JSON.stringify(rootRef);
      const types={added:'added',removed:'removed',added_to_stage:'addedToStage',removed_from_stage:'removedFromStage'};
      for(const item of events) {
        const key=JSON.stringify(item.target_ref);if(!player.object_alive(key))continue;
        const target=proxyForRef(item.target_ref);
        const route=item.route.map(ref=>{const key=JSON.stringify(ref);return player.object_alive(key)?proxyForRef(ref):proxies.get(key)?.proxy;}).filter(Boolean);
        if(item.route.some(ref=>JSON.stringify(ref)===rootKey)&&stageHost)route.push(stageHost.stage);
        const type=types[item.kind];if(!type)throw new Error(`Unsupported display lifecycle event ${item.kind}.`);
        eventHub.dispatch(target,new global.NeoFlashEvents.Event(type,type==='added'||type==='removed'),route);
      }
    }
    function drainDisplayEvents() {
      if(graphMode&&typeof player.display_events_json==='function')deliverDisplayEvents(JSON.parse(player.display_events_json()).events);
    }
    function interactiveTarget(hit) {
      let target=graphMode?proxyForRef(hit):proxyFor(hit);
      while(target&&target!==stageHost?.stage){const node=display.descriptor(target);if(node.owner!=='shape'&&node.owner!=='graphic')return target;target=display.parentSpace(target);}
      return target;
    }

    function broadcastFrame() {
      attachNewInstances();
      try {if(display){display.enterFrame();stageHost?.tick(display.nodes().map(node=>display.nodeProxy(node)));}}
      catch(error){const cause=unwrapScriptThrow(error);throw new Error(`EnterFrame callback: ${describeThrown(cause)}`,{cause});}
      dispatchScripts();
    }
    const as2 = createAs2Methods();
    const percentage = new Set(['_alpha', '_xscale', '_yscale']);
    function as2Visibility(value) {
      // AVM1's legacy display-property setter uses a number, not ToBoolean.
      // Evidence: Ruffle fe6cfc545405b888ebb478ce1c211d63147ca49b,
      // core/src/avm1/object/stage_object.rs (set_visible / property_coerce_to_number).
      // null means preserve the current value. Never invoke object conversion
      // hooks: their AVM1 semantics differ from JavaScript's Number(value).
      if (value === null || value === undefined) return null;
      if (typeof value === 'boolean') return value;
      if (typeof value === 'string') {
        // Bounded ordinary decimal mantissas; legacy radix, exponent, whitespace
        // and other parser quirks require their own compatibility implementation.
        if (value === '' || /^[+-]?[A-Za-z]+$/.test(value)) return null;
        if (!/^[+-]?(?:(?:0|[1-9][0-9]*)(?:\.[0-9]*)?|\.[0-9]+)$/.test(value)) {
          throw new TypeError('AS2 _visible string conversion currently supports plain signed decimal strings without exponent, radix, extra leading zeroes or whitespace. Pass a number or Boolean for other forms.');
        }
        value = Number(value);
      }
      if (typeof value !== 'number') {
        throw new TypeError('AS2 _visible object, function, Symbol and BigInt conversions are not supported. Pass a Boolean, number, or supported decimal string explicitly.');
      }
      return Number.isNaN(value) ? null : value !== 0;
    }
    function proxyFor(path) {
      if(graphMode){const ref=JSON.parse(player.object_ref(JSON.stringify(path)));if(!ref)throw new Error(`Display object ${JSON.stringify(path)} is no longer active.`);return proxyForRef(ref);}
      return createProxy(path,null);
    }
    function proxyForRef(objectRef) {
      if(!graphMode)throw new Error('Stable display references require the current Rust runtime. Re-export this project.');
      return createProxy(null,objectRef);
    }
    function createProxy(path,objectRef) {
      const key = JSON.stringify(objectRef||path);
      const lifetime = objectRef?objectRef.generation:player.lifetime_id(key);
      const cached = proxies.get(key);
      if (cached && cached.lifetime === lifetime) return cached.proxy;
      const live=()=>objectRef?player.object_alive(key):player.lifetime_id(key)===lifetime;
      if (!lifetime||!live()) {
        if (cached) return cached.proxy;
        throw new Error(`Display object ${key} is no longer active.`);
      }
      const call=(name,...args)=>player[objectRef?'object_'+({clip_frame:'frame',clip_frame_count:'frame_count'}[name]||name):name](key,...args);
      const custom = Object.create(null);
      function assertAlive() {
        if (!live()) throw new Error(`Display object ${key} was removed or replaced.`);
      }
      const target = Object.create(null);
      const methods = {
        play: () => control('play'),
        stop: () => control('stop'),
        gotoAndPlay: value => goto('goto_and_play', value),
        gotoAndStop: value => goto('goto_and_stop', value),
        nextFrame: () => goto('goto_and_stop', Math.min(call('clip_frame') + 2, call('clip_frame_count'))),
        prevFrame: () => goto('goto_and_stop', Math.max(call('clip_frame'), 1)),
        trace: (...values) => trace(...values),
      };
      function control(command) {assertAlive();if(!call('control',command,0))throw new Error(`Cannot control timeline at ${key}.`);}
      function goto(command, value, legacy = false) {
        assertAlive();
        const total = call('clip_frame_count');
        if (!total) throw new Error('Timeline controls apply to symbols and the root scene.');
        const frame = legacy
          ? as2Frame(value, total, label => call('resolve_frame_as2',label))
          : oneBasedFrame(value, total, label => call('resolve_frame',label));
        if (frame === null) return; // An unknown AS2 label changes neither frame nor play state.
        if (!call('control',command,frame)) throw new Error(`Cannot control timeline at ${key}.`);
      }
      const proxy = new Proxy(target, {
        get(_target, name) {
          if (name === Symbol.toStringTag) return 'NeoFlashDisplayObject';
          if (name === 'toString') return () => `[NeoFlash ${key}]`;
          if (name === 'toJSON') return () => objectRef?({object_ref:objectRef}):({path});
          if (typeof name !== 'string') return undefined;
          if (name === '_root' || name === 'root') {assertAlive();return name==='root'&&objectRef&&!display.descriptor(proxy).on_stage?null:proxyFor([]);}
          if (name === '_parent' || name === 'parent') {assertAlive();const parent=display?display.parentSpace(proxy):path.length?proxyFor(path.slice(0,-1)):stageHost?.stage||null;return name==='_parent'&&parent===stageHost?.stage?null:parent;}
          if (name === '_currentframe' || name === 'currentFrame') return call('clip_frame') + 1;
          if (name === '_totalframes' || name === 'totalFrames') return call('clip_frame_count');
          if (Object.hasOwn(methods, name)) return methods[name];
          if(display){const value=display.get(proxy,name);if(value!==display.UNHANDLED)return value;}
          if (name === 'visible' || name === '_visible') {
            assertAlive();
            return call('visible');
          }
          if (Object.hasOwn(propertyMap, name)) {
            assertAlive();
            const value = call('property',propertyMap[name]);
            return percentage.has(name) ? value * 100 : propertyMap[name]==='rotation' ? ((value%360)>180?value%360-360:(value%360)<-180?value%360+360:value%360) : value;
          }
          if(Object.hasOwn(target,name))return Reflect.get(target,name,proxy);
          if (Object.hasOwn(custom, name)) return custom[name];
          if(Reflect.has(target,name))return Reflect.get(target,name,proxy);
          if(objectRef){const child=display.children(proxy).find(node=>node.name===name);return child?display.nodeProxy(child):undefined;}
          const child = JSON.parse(player.child_path(key, name));
          return child ? proxyFor(child) : undefined;
        },
        set(_target, name, value) {
          if (typeof name !== 'string') return false;
          if(display&&display.set(proxy,name,value))return true;
          if (name === 'visible' || name === '_visible') {
            assertAlive();
            const visible = name === '_visible' ? as2Visibility(value) : Boolean(value);
            if (visible === null) return true;
            if (!call('set_visible',visible)) {
              throw new Error(`Cannot change ${name} on inactive display object ${key}.`);
            }
          } else if (Object.hasOwn(propertyMap, name)) {
            assertAlive();
            if (name.startsWith('_')) value = Number(value);
            if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error(`${name} must be a finite number.`);
            if (!call('set_property',propertyMap[name],percentage.has(name)?value/100:value)) {
              throw new Error(`Cannot change ${name} on inactive display object ${key}.`);
            }
          } else if (['currentFrame', 'totalFrames', '_currentframe', '_totalframes', '_root', '_parent', 'root', 'parent'].includes(name)) {
            throw new Error(`${name} is read-only.`);
          } else if(Object.hasOwn(target,name)||(!Object.hasOwn(custom,name)&&Reflect.has(target,name)))return Reflect.set(target,name,value,proxy);
          else custom[name] = value;
          return true;
        },
      });
      references.set(proxy,{path:path?[...path]:null,objectRef,key,lifetime});
      if(display)Object.assign(methods,display.methods(proxy));
      if(eventHub)Object.assign(methods,eventHub.methods(proxy));
      as2.register(methods.gotoAndPlay, proxy, 'gotoAndPlay', value => goto('goto_and_play', value, true));
      as2.register(methods.gotoAndStop, proxy, 'gotoAndStop', value => goto('goto_and_stop', value, true));
      proxies.set(key, { lifetime, proxy });
      return proxy;
    }
    function trace(...values) {
      console.log('[NeoFlash]', ...values);
      lastTrace = values.map(value => typeof value === 'string' ? value : String(value)).join(' ').slice(0, 160);
    }
    function dispatchScripts(clickTarget = null) {
      attachNewInstances();
      const clickTargetScope = clickTarget ? references.has(clickTarget)?clickTarget:proxyFor(clickTarget) : null;
      let count = 0;
      for (;;) {
        const events = JSON.parse(player.scripts_json());
        if (!events.length) break;
        for (const item of events) {
          if (++count > 4096 || player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive gotoAndPlay/gotoAndStop calls.');
          if (!item.script.source.trim() || !player.is_dispatch_current(JSON.stringify(item))) continue;
          const compiled = functions.get(sourceKey(item.script));
          if (!compiled) throw new Error('A script is missing from the compiled export. Re-export the project.');
          const scope = proxyFor(item.target_path);
          const event = Object.freeze({
            type: typeof item.event === 'string' ? item.event : 'frame',
            target: item.event === 'click' && clickTargetScope ? clickTargetScope : scope,
            currentTarget: scope,
          });
          try {
            compiled.fn.call(scope, event, proxyFor([]), scope._parent, trace,
              scope.play, scope.stop, scope.gotoAndPlay, scope.gotoAndStop, scope.nextFrame, scope.prevFrame, as2.method, as2.value, program, display,
              item.script.language==='action_script2'?legacyGlobals:undefined);
          } catch (error) {
            const cause = unwrapScriptThrow(error);
            throw new Error(`${compiled.location}: ${describeThrown(cause)}`, { cause });
          }
        }
        attachNewInstances();
      }
      if (player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive timeline controls.');
    }
    function resize() { preview.resize(); }
    function font(kind) { return `${kind.font_size}px ${JSON.stringify(kind.font_family)}, sans-serif`; }
    function render() {
      const snapshot=graphMode?JSON.parse(player.runtime_snapshot_json()):null;
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      context.setTransform(canvas.width / project.width, 0, 0, canvas.height / project.height, 0, 0);
      context.clearRect(0, 0, project.width, project.height);
      context.fillStyle = color(project.background);
      context.fillRect(0, 0, project.width, project.height);
      for (const item of scene) {
        const kind = item.kind;
        context.save();
        context.transform(...item.transform);
        context.globalAlpha = Math.max(0, Math.min(1, item.alpha));
        if (kind.type === 'rectangle' || kind.type === 'ellipse') {
          context.beginPath();
          if (kind.type === 'rectangle') context.rect(0, 0, kind.width, kind.height);
          else context.ellipse(kind.width / 2, kind.height / 2, Math.max(0, kind.width / 2), Math.max(0, kind.height / 2), 0, 0, Math.PI * 2);
          context.fillStyle = color(kind.fill);
          context.fill();
          if (kind.stroke_width > 0 && kind.stroke[3]) {
            context.strokeStyle = color(kind.stroke);
            context.lineWidth = kind.stroke_width;
            context.stroke();
          }
        } else if (kind.type === 'path') {
          const path = paths.get(kind.commands);
          if (kind.fill[3] > 0) {
            context.fillStyle = color(kind.fill);
            context.fill(path, kind.fill_rule || 'nonzero');
          }
          if (kind.stroke_width > 0 && kind.stroke[3] > 0) {
            context.strokeStyle = color(kind.stroke);
            context.lineWidth = kind.stroke_width;
            context.lineCap = 'round';
            context.lineJoin = 'round';
            context.stroke(path);
          }
        } else if (kind.type === 'text') {
          context.font = font(kind);
          context.textBaseline = 'top';
          context.fillStyle = color(kind.color);
          kind.text.split('\n').forEach((line, index) => context.fillText(line, 0, index * kind.font_size * 1.2));
        } else throw new Error(`Unsupported display primitive: ${kind.type}`);
        context.restore();
      }
      const semanticScene=snapshot?scene.map(item=>{let node=snapshot.objects.find(node=>JSON.stringify(node.object_ref)===JSON.stringify(item.owner_ref));while(node&&node.authored_path===null)node=snapshot.objects.find(parent=>JSON.stringify(parent.object_ref)===JSON.stringify(node.parent_ref));return {...item,instance_path:node?.authored_path||[]};}):scene;
      const targetCount = semanticLayer.update(snapshot?snapshot.semantics:JSON.parse(player.accessibility_json()), semanticScene, project, preview.view.scale, (line, kind) => { context.font = font(kind); return context.measureText(line).width; });
      if (targetCount) canvas.setAttribute('aria-hidden', 'true'); else canvas.removeAttribute('aria-hidden');
      status.textContent = `${player.current_frame() + 1} / ${player.total_frames()} · ${runtimeFrameRate} fps`;
      if (traceElement.textContent !== lastTrace) traceElement.textContent = lastTrace;
      toggle.textContent = player.is_playing() ? 'Pause' : 'Play';
    }
    function hitAt(event) {
      scene = graphMode?JSON.parse(player.runtime_snapshot_json()).draws:JSON.parse(player.scene_json());
      const rect = canvas.getBoundingClientRect();
      const x = (event.clientX - rect.left) * project.width / rect.width;
      const y = (event.clientY - rect.top) * project.height / rect.height;
      for (let index = scene.length - 1; index >= 0; index--) {
        const item = scene[index];
        if (item.alpha <= 0) continue;
        const local = localPoint(item.transform, x, y);
        if (!local) continue;
        if (hitShape(item.kind, ...local, (line, kind) => {
          context.font = font(kind);
          return context.measureText(line).width;
        }, (kind, x, y) => global.NeoFlashPaths.contains(context, kind, x, y, paths.get(kind.commands)))) return graphMode?item.owner_ref:item.instance_path;
      }
      return null;
    }
    canvas.addEventListener('click', event => {
      if (failed || preview.panMode || preview.suppressClick) return;
      try {
        const hit=hitAt(event),target=hit?(graphMode?proxyForRef(hit):proxyFor(hit)):null;
        stageHost?.handlePointer('click',event);
        if(target&&display.alive(target)){
          let owner=target,node=display.descriptor(owner);
          while(graphMode&&node.authored_path===null){owner=display.parentSpace(owner);if(!owner||owner===stageHost?.stage)break;node=display.descriptor(owner);}
          const path=graphMode?node.authored_path:node.path;
          if(path)player.click(JSON.stringify(path));
          dispatchScripts(target);
          render();
        }

      } catch (error) { fail(error); }
    });
    canvas.addEventListener('pointermove', event => { pointerOverContent=Boolean(hitAt(event));updateCursor(); });
    canvas.addEventListener('pointerleave', () => { pointerOverContent=false;updateCursor(); });
    toggle.addEventListener('click', () => {
      player.set_playing(!player.is_playing());
      lastTime = null;
      accumulated = 0;
      render();
    });
    document.getElementById('restart').addEventListener('click', () => {
      failed = false;
      errorElement.style.display = 'none';
      accumulated = 0; lastTime = null;
      try { resetProject(); render(); } catch (error) { fail(error); }
    });
    global.addEventListener('resize', () => { resize(); render(); });
    document.addEventListener('visibilitychange', () => { lastTime = null; accumulated = 0; });
    document.getElementById('zoom').addEventListener('change', event => preview.setMode(event.target.value));
    const panButton = document.getElementById('pan');
    panButton.addEventListener('click', () => { preview.setPan(!preview.panMode); panButton.setAttribute('aria-pressed', String(preview.panMode)); updateCursor(); });
    global.addEventListener('pagehide', event => { if(!event.persisted)disposeFamily(); });
    new ResizeObserver(() => { resize(); render(); }).observe(viewport);
    stopAsyncOnFailure=()=>{stopTimers();binaryHost?.reset();};
    initializeFamily();
    canvas.tabIndex=-1;
    stageHost?.attach();
    resize();
    // Startup script failures use the same stopped-but-restartable policy as
    // later events. Keep installing the animation loop after reporting them.
    try { dispatchScripts(); render(); } catch (error) { fail(error); }
    document.getElementById('loading').style.display = 'none';
    function animate(now) {
      if (!failed && !document.hidden) {
        try {
          if (lastTime !== null && player.is_playing()) {
            accumulated += Math.min(now - lastTime, 250);
            const interval = 1000 / runtimeFrameRate;
            let ticks = 0;
            while (accumulated >= interval && ticks++ < 12) {
              accumulated -= interval;
              broadcastFrame();
              player.tick();
              dispatchScripts();
            }
            // Avoid a runaway catch-up after a suspended browser tab.
            if (ticks >= 12) accumulated = 0;
          }
          lastTime = now;
          render();
        } catch (error) { fail(error); }
      }
      global.requestAnimationFrame(animate);
    }
    global.requestAnimationFrame(animate);
    global.NeoFlashPlayer = Object.freeze({ get root() { return proxyFor([]); }, pause: () => player.set_playing(false), resume: () => player.set_playing(true), restart: () => document.getElementById('restart').click() });
  }
  boot().catch(fail);
})(globalThis);
