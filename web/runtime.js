/* NeoFlash HTML5 host. Timeline evaluation and display-list construction run in shared Rust/WASM. */
(function (global) {
  'use strict';

  function localPoint(matrix, x, y) {
    const [a, b, c, d, tx, ty] = matrix;
    const determinant = a * d - b * c;
    if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-12) return null;
    return [(d * (x - tx) - c * (y - ty)) / determinant, (-b * (x - tx) + a * (y - ty)) / determinant];
  }
  function hitShape(kind, x, y, measureText, testPath, testBitmap, testText) {
    // Runtime colors already include the complete ColorTransform. A leaf can
    // have draw.alpha=1 while every actual painted channel is transparent.
    if ((kind.type==='rectangle'||kind.type==='ellipse'||kind.type==='path')&&
        (kind.fill?.[3]??255)<=0&&(!(kind.stroke?.[3]>0)||!(kind.stroke_width>0))) return false;
    if (kind.type === 'rectangle') return x >= 0 && y >= 0 && x <= kind.width && y <= kind.height;
    if (kind.type === 'ellipse') {
      if (kind.width <= 0 || kind.height <= 0) return false;
      return Math.pow((x - kind.width / 2) / (kind.width / 2), 2) + Math.pow((y - kind.height / 2) / (kind.height / 2), 2) <= 1;
    }
    if (kind.type === 'text') {
      if ((kind.color?.[3]??255)<=0) return false;
      const lines = kind.text.split('\n');
      return x >= 0 && y >= 0 && y <= lines.length * kind.font_size * 1.2 &&
        x <= Math.max(0, ...lines.map(line => measureText(line, kind)));
    }
    if (kind.type === 'path' || kind.type === 'gradient_path') return testPath(kind, x, y);
    if (kind.type === 'bitmap_pixels') return testBitmap?testBitmap(kind,x,y):false;
    if (kind.type === 'video_frame') return Boolean(kind.alpha_bounds)&&x>=0&&y>=0&&x<kind.width&&y<kind.height;
    if (kind.type === 'text_field') return testText?testText(kind.field,x,y):false;
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
    const variants = new WeakMap();let resolve=(receiver,name)=>receiver[name];
    function aliasError() {
      return new TypeError('AS2 goto method aliases need receiver semantics not supported yet. Call gotoAndPlay/gotoAndStop directly on the target timeline.');
    }
    return {
      register(method, receiver, name, implementation) { variants.set(method, { receiver, name, implementation }); },
      setResolver(resolver) { resolve=resolver; },
      method(receiver, name) {
        // Resolve the getter before arguments, then check callability after
        // argument evaluation, preserving ordinary call evaluation order.
        const method = resolve(receiver,name);
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
  // The Stage and BitmapData.draw share this exact painter. Rust supplies
  // the authoritative snapshot; this adapter never reconstructs a scene graph.
  function createScenePainter({global,paths,gradients,clippings,bitmapHost,textHost,filterHost,videoHost,context,matrix=item=>item.transform,hidden=()=>false,snapBitmaps=true}) {
    const bitmapMatrix=matrix;
    const font=kind=>`${kind.font_size}px ${JSON.stringify(kind.font_family)}, sans-serif`;
    function paintItem(context,item) {
        const kind = item.kind;
        if(kind.type==='text_field'&&hidden(item.owner_ref))return;
        context.save();
        try {
        context.transform(...bitmapMatrix(item));
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
        } else if (kind.type === 'path' || kind.type === 'gradient_path') {
          gradients.draw(context,kind,paths.get(kind.commands),bitmapHost?.cache);
        } else if (kind.type === 'bitmap_pixels') {
          if(!bitmapHost)throw new Error('Bitmap runtime is missing. Re-export this project.');
          bitmapHost.cache.draw(context,snapBitmaps?kind:{...kind,pixel_snapping:'never'});
        } else if (kind.type === 'video_frame') {
          if(!videoHost)throw new Error('Video runtime is missing. Re-export this project.');
          videoHost.draw(context,kind,item.owner_ref);
        } else if (kind.type === 'text_field') {
          if(!textHost)throw new Error('Text layout runtime is missing. Re-export this project.');
          textHost.draw(context,kind.field);
        } else if (kind.type === 'text') {
          context.font = font(kind);
          context.textBaseline = 'top';
          context.fillStyle = color(kind.color);
          kind.text.split('\n').forEach((line, index) => context.fillText(line, 0, index * kind.font_size * 1.2));
        } else throw new Error(`Unsupported display primitive: ${kind.type}`);
        } finally { context.restore(); }
    }
    function paintBounds(item,geometry=false) {
      let kind=item.kind.type==='text_field'?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind;
      if(geometry){
        const opaque=rgba=>[...(rgba||[0,0,0,0]).slice(0,3),255];
        if(['rectangle','ellipse','path','gradient_path'].includes(kind.type))kind={...kind,type:kind.type==='gradient_path'?'path':kind.type,fill:opaque(kind.fill),stroke:opaque(kind.stroke)};
        else if(kind.type==='bitmap_pixels'||kind.type==='video_frame')kind={...kind,alpha_bounds:{left:0,top:0,right:kind.width,bottom:kind.height}};
        else if(kind.type==='text_field')kind={...kind,field:{...kind.field,alpha:255},text_bounds:kind.field.box?{left:0,top:0,right:kind.field.box.width,bottom:kind.field.box.height}:textHost.bounds({...kind.field,alpha:255},true)};
        else if(kind.type==='text')kind={...kind,color:opaque(kind.color)};
      }
      let bounded=global.NeoFlashAccessibility.itemBounds({...item,alpha:geometry?1:item.alpha,kind,transform:bitmapMatrix(item)},(line,textKind)=>{context.font=font(textKind);return context.measureText(line).width;});
      // Scratch bounds also include actual glyph overhang; layout advance
      // widths alone could crop italic text before applying the real mask.
      if((kind.type==='text_field'&&kind.field.alpha>0)||(kind.type==='text'&&kind.color[3]>0)){
        const field=kind.type==='text_field'?kind.field:null,lines=field?textHost.layout(field).lines:kind.text.split('\n').map((text,index)=>({text,x:0,y:index*kind.font_size*1.2})),m=bitmapMatrix(item);
        context.save();try{context.font=field?global.NeoFlashText.cssFont(field.format):font(kind);context.textAlign='left';context.textBaseline='top';
          for(const line of lines){if(!line.text.trim())continue;const metric=context.measureText(line.text),values=[metric.actualBoundingBoxLeft,metric.actualBoundingBoxRight,metric.actualBoundingBoxAscent,metric.actualBoundingBoxDescent];if(!values.every(Number.isFinite))continue;
            let left=line.x-values[0],right=line.x+values[1],top=line.y-values[2],bottom=line.y+values[3];if(field?.box){left=Math.max(0,left);top=Math.max(0,top);right=Math.min(field.box.width,right);bottom=Math.min(field.box.height,bottom);}if(right<=left||bottom<=top)continue;
            for(const[x,y]of [[left,top],[right,top],[right,bottom],[left,bottom]]){const px=m[0]*x+m[2]*y+m[4],py=m[1]*x+m[3]*y+m[5];bounded=bounded?{left:Math.min(bounded.left,px),top:Math.min(bounded.top,py),right:Math.max(bounded.right,px),bottom:Math.max(bounded.bottom,py)}:{left:px,top:py,right:px,bottom:py};}
          }
        }finally{context.restore();}
      }
      return bounded;
    }
    function draw(context,snapshot,{width=context.canvas.width,height=context.canvas.height,semantic=true}={}) {
      const scene=snapshot.draws;
      clippings.beginFrame();
      const clips=global.NeoFlashPaths.clipTable(snapshot?.clips||[]),objects=snapshot?new Map((snapshot.objects||[]).map(node=>[JSON.stringify(node.object_ref),node])):null;
      const semanticOwners=new Set((semantic?snapshot?.semantics||[]:[]).map(node=>JSON.stringify(node.owner_ref))),needsCoverage=new Map(),clippedBounds=new Map();
      const semanticCoverage=item=>{const key=JSON.stringify(item.owner_ref);if(needsCoverage.has(key))return needsCoverage.get(key);let needed=false;for(let node=objects?.get(key);node;node=objects.get(JSON.stringify(node.parent_ref)))if(semanticOwners.has(JSON.stringify(node.object_ref))){needed=true;break;}needsCoverage.set(key,needed);return needed;};
      const paintBoundsCache=new Map(),geometryBoundsCache=new Map();
      const bounded=(item,geometry=false)=>{const cache=geometry?geometryBoundsCache:paintBoundsCache;if(!cache.has(item))cache.set(item,paintBounds(item,geometry));return cache.get(item);};
      const paintLeaf=(target,item,deferred=[])=>{
        const clip_ids=(item.clip_ids||[]).filter(id=>!deferred.includes(id));
        if(!clip_ids.length){paintItem(target,item);return;}
        const readBounds=!item.filter_id&&semanticCoverage(item);
        const visible=clippings.draw(target,{...item,clip_ids},clips,paths,canvas=>paintItem(canvas,item),{bounds:bounded(item),readBounds});
        if(readBounds)clippedBounds.set(item,visible||null);
      };
      const composite=(target,surface,bounds,clip_ids,operation='source-over')=>{
        target.save();try{target.globalCompositeOperation=operation;const draw=canvas=>canvas.drawImage(surface,bounds.left,bounds.top);
          if(clip_ids.length)clippings.draw(target,{clip_ids},clips,paths,draw,{bounds});else draw(target);
        }finally{target.restore();}
      };
      if(snapshot?.filter_groups?.length){
        if(!filterHost)throw new Error('Display filters require the shared filter runtime. Re-export this project.');
        filterHost.renderer.draw(context,snapshot,{bounds:item=>bounded(item,true),paint:paintLeaf,composite});
        // Filter effects are not new interaction geometry. Read the original
        // fully clipped source only for semantic ancestors, even on cache hits.
        for(const item of scene)if(item.filter_id&&item.clip_ids?.length&&semanticCoverage(item)){
          const area=bounded(item),visible=area&&{left:Math.max(0,area.left),top:Math.max(0,area.top),right:Math.min(width,area.right),bottom:Math.min(height,area.bottom)};
          const result=filterHost.renderer.coverage(visible,target=>clippings.draw(target,item,clips,paths,canvas=>paintItem(canvas,item),{bounds:visible,readBounds:true}));
          clippedBounds.set(item,result||null);
        }
      }else{
        filterHost?.renderer.clear();
        for(const item of scene)paintLeaf(context,item);
      }
      return {clippedBounds,objects};
    }
    function clear(){gradients.clear();clippings.clear();}
    return {draw,paintItem,paintBounds,clear};
  }
  const helpers = { createScenePainter, localPoint, hitShape, oneBasedFrame, as2Frame, createAs2Methods, sourceKey, describeThrown, unwrapScriptThrow };
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
    // Published bundles carry a flattened main timeline plus its scene ranges.
    const player = new NeoFlashWasm.WebPlayer(JSON.stringify(project), bundle.scenes ? JSON.stringify(bundle.scenes) : undefined);
    const canvas = document.getElementById('stage');
    const context = canvas.getContext('2d');
    if (!context) throw new Error('This browser does not provide Canvas 2D.');
    if (!global.NeoFlashPaths) throw new Error('The vector path runtime is missing. Re-export this project.');
    const paths = new global.NeoFlashPaths.PathCache();
    const gradients = new global.NeoFlashPaths.GradientRenderer();
    const clippings = new global.NeoFlashPaths.ClipRenderer();
    const status = document.getElementById('status');
    const toggle = document.getElementById('toggle');
    const traceElement = document.getElementById('trace');
    const viewport = document.getElementById('viewport');
    const semanticLayer = new global.NeoFlashAccessibility.SemanticLayer(document.getElementById('semantic-layer'), node => {
      if (failed) return;
      try {activateSemantic(node);} catch (error) {fail(error);}
    });
    const preview = new global.NeoFlashPreview.Preview(viewport, document.getElementById('stage-space'), document.getElementById('stage-surface'), canvas, project, () => render());
    const functions = new Map();
    const proxies = new Map();
    let scene = [];
    let lastTime = null;
    let accumulated = 0;
    let lastTrace = '';
    const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame', '__nfAs2Method', '__nfAs2Value', '__nfProgram', '__nfDisplay', '__nfLegacy', '__nfXML'];
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
    const registeredClasses = new Map();
    const executedInitClips = new Set();
    let callFrameDepth = 0, inDispatch = false;
    let display = null, eventHub = null, stageHost = null, legacyHost = null, legacyGlobals = null, timerHost = null, binaryHost = null, netHost = null, bitmapHost = null, textHost = null, filterHost = null, audioHost = null, videoHost = null, xmlHost = null, loaderHost = null, amfHost = null, storageHost = null, textInputHost = null, externalHost = null, program = null, as2PlatformHost = null, as2ObjectHost = null, as2TextHost = null, as2DataHost = null, as2BindingHost = null, as2NetworkHost = null, as2FilesHost = null, as2LocaleHost = null, as2MediaHost = null, as2ServicesHost = null, as2TransitionsHost = null, as2DepthManagerHost = null, as2StageHost = null, as2CaptureHost = null, as2ConnectionsHost = null, as2BoxingHost = null, as2PrintHost = null;
    let geometryHost = null, geometry3dHost = null, easingHost = null, broadcasterHost = null;
    let frameLabelType = null, sceneValueFactory = null;
    const timelineMetadataProperties=Object.freeze({currentLabel:'current_label',currentFrameLabel:'current_frame_label',currentLabels:'labels',framesLoaded:'frames_loaded',isPlaying:'is_playing',currentScene:'scene_name',scenes:'scene_name'});
    let eventFamilies = {};
    let timerRenderGeneration=0, capturePainter=null,inputPaths=null;
    let legacyCursorVisible = true, pointerOverContent = false;
    let runtimeFrameRate = typeof project === 'undefined' ? 24 : project.fps;
    function logicalSize(){return [typeof preview==='undefined'?typeof project==='undefined'?800:project.width:preview.view.logicalWidth||project.width,typeof preview==='undefined'?typeof project==='undefined'?600:project.height:preview.view.logicalHeight||project.height];}
    function updateCursor() {
      if(typeof canvas!=='undefined')canvas.style.cursor=!legacyCursorVisible?'none':typeof preview!=='undefined'&&preview.panMode?'grab':pointerOverContent?'pointer':'default';
    }
    function inputFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Input callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else throw failure;
    }
    function stopTimers() {timerRenderGeneration++;easingHost?.reset?.();broadcasterHost?.reset?.();timerHost?.reset();}
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
    function netFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Resource callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {externalHost?.reset();textInputHost?.reset();storageHost?.reset();loaderHost?.reset();xmlHost?.reset();videoHost?.reset();as2CaptureHost?.reset();audioHost?.reset();netHost?.reset();stopTimers();throw failure;}
    }
    function audioFailure(error){
      const cause=unwrapScriptThrow(error),failure=new Error(`Audio callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {audioHost?.reset();throw failure;}
    }
    function updateAudioStatus(){
      if(typeof document==='undefined')return;
      const states=[audioHost?.status(),videoHost?.status()].filter(value=>value&&value.state!=='idle');
      const state={state:states.some(value=>value.state==='awaiting_gesture')?'awaiting_gesture':states.some(value=>value.state==='loading')?'loading':states.length?'ready':'idle',unlocked:states.every(value=>value.unlocked),muted:states.length>0&&states.every(value=>value.muted),pending:states.reduce((n,value)=>n+value.pending,0),active:states.reduce((n,value)=>n+value.active,0),loading:states.reduce((n,value)=>n+value.loading,0),error:states.find(value=>value.error)?.error};
      const button=document.getElementById('audio-toggle'),label=document.getElementById('audio-status');if(!button||!label)return;
      button.hidden=label.hidden=state.state==='idle';
      button.textContent=!state.unlocked||state.state==='awaiting_gesture'?'Enable audio':state.muted?'Unmute audio':'Mute audio';
      button.setAttribute('aria-pressed',String(state.muted));
      label.textContent=state.state==='awaiting_gesture'?`Audio waiting for a gesture · ${state.pending} pending`:state.error?`Audio: ${state.error}`:state.state==='loading'?`Loading audio · ${state.loading}`:state.muted?'Audio muted':state.active?`Audio playing · ${state.active}`:'Audio ready';
    }
    function assertUnfilteredEditing(target,addingTo=null){
      if(!target||target===stageHost?.stage||!display?.alive(target))return;
      const nodes=display.nodes(),node=display.descriptor(target,nodes);if(node.owner!=='text'||(node.text?.field_type!=='input'&&node.text?.selectable===false))return;
      const addedKey=addingTo?references.get(addingTo)?.key:null,byRef=new Map(nodes.map(node=>[JSON.stringify(node.object_ref),node]));
      const rasterRefs=new Set(graphMode?(JSON.parse(player.runtime_snapshot_json()).filter_groups||[]).map(group=>JSON.stringify(group.owner_ref)):[]);
      for(let current=node;current;current=byRef.get(JSON.stringify(current.parent_ref))){
        if(current.filters?.length||rasterRefs.has(JSON.stringify(current.object_ref))||JSON.stringify(current.object_ref)===addedKey)throw new TypeError('Native TextField editing inside a filtered, cached or blended subtree is not supported. Clear its raster effects or move focus before editing.');
      }
    }
    function legacyNavigationVariables(owner){
      const ref=references.get(owner);if(!ref)return owner;
      display.descriptor(owner);
      const result=Object.create(null),keys=new Set([...(ref.customKeys?.()||[]),...(program?.jsonKeys?.(owner)||[])]);
      for(const name of keys){
        const value=owner[name];
        // Methods and child-display references are not MovieClip form vars.
        // Other non-data values still reach the transport's explicit validator.
        if(typeof value==='function'||(value!==null&&typeof value==='object'&&references.has(value)))continue;
        Object.defineProperty(result,name,{value,enumerable:true,configurable:true,writable:true});
      }
      return result;
    }
    function initializeFamily() {
      if (!global.NeoFlashDisplay || typeof player.display_nodes_json !== 'function') return;
      const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;
      if(global.NeoFlashAS2Platform)as2PlatformHost=global.NeoFlashAS2Platform.create(wasm,{global,document:global.document,onError:inputFailure,requestRender:requestTimerRender});
      eventFamilies=global.NeoFlashEvents?.createFamilies?.(wasm,{isByteArray:value=>Boolean(binaryHost?.ByteArray.__nfIs(value)),requestRender:requestTimerRender})||{};
      geometryHost=global.NeoFlashGeom.create(wasm,{getVector3D:()=>geometry3dHost.Vector3D});
      if(global.NeoFlashBitmap)bitmapHost=global.NeoFlashBitmap.create({player,geometry:geometryHost,as2,
        getBinary:()=>binaryHost,getFilters:()=>filterHost,
        displayRef:source=>{const ref=references.get(source);if(!ref)return null;display.descriptor(source);return ref.objectRef;},
        paintScene:(target,snapshot)=>{
          if(!global.NeoFlashPaths)throw new Error('BitmapData.draw requires the shared path renderer.');
          if(!capturePainter)capturePainter=createScenePainter({global,paths:new global.NeoFlashPaths.PathCache(),
            gradients:new global.NeoFlashPaths.GradientRenderer({createCanvas:()=>bitmapHost.createCanvas(1,1)}),
            clippings:new global.NeoFlashPaths.ClipRenderer({createCanvas:()=>bitmapHost.createCanvas(1,1)}),bitmapHost,textHost,filterHost,videoHost,context:target,snapBitmaps:false});
          snapshot=videoHost?videoHost.prepareSnapshot(snapshot):snapshot;
          bitmapHost.cache.beginFrame();try{capturePainter.draw(target,snapshot,{semantic:false});}finally{bitmapHost.cache.endFrame();}
        }});
      if(global.NeoFlashFilters&&typeof player.filter_rgba==='function')filterHost=global.NeoFlashFilters.create({player,getBitmap:()=>bitmapHost,geometry:geometryHost});
      if(global.NeoFlashText&&typeof player.object_text_info==='function')textHost=global.NeoFlashText.create({player,onUpdate:ref=>textInputHost?.sync(ref),measureText:(text,format)=>{if(typeof context==='undefined')throw new Error('Text layout requires the Canvas font measurement service.');context.save();try{global.NeoFlashText.applyFont(context,format,true);return context.measureText(text).width;}finally{context.restore();}}});
      if(textHost&&global.NeoFlashAS2Text)as2TextHost=global.NeoFlashAS2Text.create({broadcaster:global.NeoFlashBroadcaster,wasm,player,text:textHost,getNet:()=>netHost,targetForRef:key=>proxyForRef(JSON.parse(key)),afterCallback:afterTimerEvent,project:typeof project==='undefined'?null:project,document:typeof document==='undefined'?null:document});
      display=global.NeoFlashDisplay.create({player,geometry:geometryHost,proxyFor,proxyForRef,onMutation:deliverDisplayEvents,project:typeof project==='undefined'?undefined:project,prepareLinked,getBitmap:()=>bitmapHost,getText:()=>textHost,getAS2Text:()=>as2TextHost,getLoader:()=>loaderHost,getVideo:()=>videoHost,getFilters:()=>filterHost,getNet:()=>netHost,as2,
        shapeContains:(kind,x,y)=>{if(typeof context==='undefined'||typeof context.isPointInPath!=='function')throw new Error('Shape hit testing requires Canvas Path2D support.');inputPaths??=new global.NeoFlashPaths.PathCache();return global.NeoFlashPaths.contains(context,kind,x,y,inputPaths.get(kind.commands));},
        clipsContain:(snapshot,ids,x,y)=>{if(typeof context==='undefined')throw new Error('Clipped input requires Canvas Path2D support.');inputPaths??=new global.NeoFlashPaths.PathCache();return global.NeoFlashPaths.clipsContain(context,ids,global.NeoFlashPaths.clipTable(snapshot.clips||[]),x,y,inputPaths);},
        beforeRaster:target=>assertUnfilteredEditing(stageHost?.stage.focus,target),beforeFilters:(target,specs)=>{if(specs.length)assertUnfilteredEditing(stageHost?.stage.focus,target);},referenceOf:target=>references.get(target),getStage:()=>stageHost?.stage||null,
        measureText:(line,kind)=>{if(typeof context==='undefined')throw new Error('Text bounds require the Canvas font measurement service.');context.font=font(kind);return context.measureText(line).width;},
        pathBounds:(commands,matrix)=>global.NeoFlashAccessibility.pathBounds(commands,matrix)});
      if(global.NeoFlashEvents&&global.NeoFlashStage) {
        const alive=target=>target===stageHost?.stage||display.alive(target);
        const local=(target,x,y)=>{const p=target===stageHost?.stage?{x,y}:display.convert(target,{x,y},false);return[p.x,p.y];};
        eventHub=global.NeoFlashEvents.create({alive,parentOf:target=>target===stageHost?.stage?null:target.parent,
          localPoint:local,globalPoint:(target,x,y)=>{const p=target===stageHost?.stage?{x,y}:display.convert(target,{x,y},true);return[p.x,p.y];}});
        stageHost=global.NeoFlashStage.create({events:eventHub,width:typeof project==='undefined'?800:project.width,height:typeof project==='undefined'?600:project.height,
          frameRate:runtimeFrameRate,getFrameRate:()=>runtimeFrameRate,setFrameRate:value=>{runtimeFrameRate=value;},alive:target=>alive(target)&&(target===stageHost?.stage||display.descriptor(target).on_stage!==false),localPoint:local,
          canvas:typeof canvas==='undefined'?null:canvas,keyboardTarget:typeof viewport==='undefined'?null:viewport,pointerTarget:global,
          parentOf:target=>display.parentSpace(target),resolveInput:target=>display.resolveInput(target),beforePointer:()=>display.updateDrag(),hasDrag:()=>display.hasDrag(),cancelPointer:()=>display.cancelPointer(),
          onPointerTarget:target=>{pointerOverContent=display.handCursor(target);updateCursor();},
          onFocusChanged:(next,previous)=>textInputHost?.focusChanged(next,previous),ownsTextInput:node=>textInputHost?.owns(node),canFocus:value=>{if(textInputHost&&!textInputHost.canFocus(value))return false;assertUnfilteredEditing(value);return true;},
          hitTest:event=>{if(typeof canvas==='undefined')return null;const rect=canvas.getBoundingClientRect(),[width,height]=logicalSize(),x=(event.clientX-rect.left)*width/rect.width,y=(event.clientY-rect.top)*height/rect.height;return x>=0&&y>=0&&x<=width&&y<=height?display.pick(x,y):null;},
          coordinates:event=>{if(typeof canvas==='undefined')return null;const r=canvas.getBoundingClientRect(),[width,height]=logicalSize();return[(event.clientX-r.left)*width/r.width,(event.clientY-r.top)*height/r.height];},
          isInputEnabled:()=>typeof failed==='undefined'||(!failed&&!preview.panMode&&!preview.suppressClick),
          onLegacyInput:(type,target,event)=>display.pointer(type,target,event),
          afterInput:()=>{dispatchScripts();if(typeof context!=='undefined')render();},onError:inputFailure});
        if(global.NeoFlashLegacyInput){
          legacyHost=global.NeoFlashLegacyInput.create({stage:stageHost.stage,platform:as2PlatformHost,
            keyTarget:typeof global.addEventListener==='function'?global:null,
            alive:listener=>!references.has(listener)||display.alive(listener),
            onVisibility:visible=>{legacyCursorVisible=visible;updateCursor();},onError:inputFailure});
          if(global.NeoFlashAS2Stage)as2StageHost=global.NeoFlashAS2Stage.create({wasm,legacy:legacyHost,geometry:geometryHost,width:typeof project==='undefined'?800:project.width,height:typeof project==='undefined'?600:project.height,preview:typeof preview==='undefined'?null:preview,viewport:typeof viewport==='undefined'?null:viewport,canvas:typeof canvas==='undefined'?null:canvas,display,getRoot:()=>proxyFor([]),contextTarget:event=>hitAt(event,true)?.target||proxyFor([]),onError:inputFailure,afterEvent:afterTimerEvent});
          legacyGlobals=Object.freeze({Key:legacyHost.Key,Mouse:legacyHost.Mouse,Stage:as2StageHost?.Stage||legacyHost.Stage,...(as2StageHost?{ContextMenu:as2StageHost.ContextMenu,ContextMenuItem:as2StageHost.ContextMenuItem}:{})});
        }
      }
      legacyGlobals=Object.freeze({...legacyGlobals,_global:Object.create(null),newline:"\n",caseKey(receiver,name){
        if(typeof wasm.legacy_case_name_json!=='function')throw new Error('The SWF6 case-name service is missing. Re-export this project.');
        const keys=[],seen=new Set(),add=key=>{if(typeof key==='string'&&!seen.has(key)){if(keys.length>=100000)throw new RangeError('Legacy property lookup exceeds 100000 names.');seen.add(key);keys.push(key);}};
        const ref=references.get(receiver);
        if(ref){for(const key of ref.nativeKeys())add(key);for(const key of ref.customKeys())add(key);for(const node of display.children(receiver))add(node.name);}
        for(let object=Object(receiver),depth=0;object;object=Object.getPrototypeOf(object),depth++){
          if(depth>=256)throw new RangeError('Legacy property prototype lookup exceeds 256 levels.');
          for(const key of Object.getOwnPropertyNames(object))add(key);
        }
        return wasm.legacy_case_name_json(name,JSON.stringify(keys));
      }});
      if(as2PlatformHost)legacyGlobals=Object.freeze({...legacyGlobals,System:as2PlatformHost.System,getVersion:as2PlatformHost.getVersion,updateAfterEvent:as2PlatformHost.updateAfterEvent});
      function legacyProperty(target, index) {
        if(!Number.isInteger(Number(index))||Number(index)<0||Number(index)>21)throw new RangeError('AS2 property index must be between 0 and 21.');
        const receiver=display.resolveTarget(target,proxyFor([])),name=wasm.as2_property_name(Number(index));
        const native=legacyGlobals.nativeRef?.(receiver,name);
        if(native)return {receiver,name,native};
        if(!references.get(receiver)?.nativeHas(name))throw new Error('AS2 getProperty/setProperty does not yet support '+name+'.');
        return {receiver,name};
      }
      function updateAs2Accessibility(){
        if(typeof player.as2_accessibility_update!=='function'||!display)return;
        const entries=[];
        for(const node of display.nodes()){
          if(!['movie_clip','button','text'].includes(node.owner))continue;
          const cached=proxies.get(JSON.stringify(node.object_ref));if(!cached)continue;
          const p=cached.proxy,acc=display.readAccProps?.(p);
          if(!acc||typeof acc!=='object')continue;
          const interactive=node.owner==='button'||['onPress','onRelease','onReleaseOutside','onRollOver','onRollOut','onDragOver','onDragOut'].some(k=>typeof p[k]==='function');
          entries.push({object_ref:node.object_ref,props:{silent:Boolean(acc.silent),force_simple:Boolean(acc.forceSimple),name:typeof acc.name==='string'?acc.name:null,description:typeof acc.description==='string'?acc.description:null,interactive}});
        }
        player.as2_accessibility_update(JSON.stringify(entries));
        if(typeof semanticLayer!=='undefined'&&semanticLayer?.update&&graphMode){
          const snap=JSON.parse(player.runtime_snapshot_json()),byRef=new Map(snap.objects.map(n=>[JSON.stringify(n.object_ref),n]));
          const semScene=snap.draws.map(item=>{const ancestry_refs=[];for(let n=byRef.get(JSON.stringify(item.owner_ref));n;n=byRef.get(JSON.stringify(n.parent_ref)))ancestry_refs.push(n.object_ref);return {...item,ancestry_refs,kind:item.kind.type==='text_field'&&textHost?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind};});
          const [w,h]=logicalSize(),sc=typeof preview==='undefined'?1:{x:preview.view.scale,y:preview.view.scaleY||preview.view.scale};
          semanticLayer.update(snap.semantics.filter(node=>Boolean(semanticRoute(node,snap.objects))),semScene,{width:w,height:h},sc,(line,kind)=>{if(typeof context==='undefined')return line.length*kind.font_size*0.6;context.font=font(kind);return context.measureText(line).width;});
        }
        if(!inDispatch&&typeof context!=='undefined')render();
      }
      const legacyDisplayCall=(target,method,args)=>as2.method(display.resolveTarget(target,proxyFor([])),method)(...args);
      legacyGlobals=Object.freeze({...legacyGlobals,
        __nfCallStack:[],
        MovieClip:display.builtins['flash.display.MovieClip'],
        Button:display.builtins['flash.display.SimpleButton'],
        Accessibility:Object.freeze({
          isActive:()=>Boolean(typeof semanticLayer!=='undefined'&&semanticLayer?.container),
          updateProperties:()=>updateAs2Accessibility(),
        }),
        level:n=>n===0?proxyFor([]):undefined,
        levelRef:n=>({has:()=>n===0,delete:()=>false,get:()=>n===0?proxyFor([]):undefined,set:()=>{throw new TypeError('AS2 _level references are read-only.');}}),
        resolveTarget:(value,relative)=>display.resolveTarget(value,relative??proxyFor([])),
        runInitClip:(key,root,fn)=>{if(executedInitClips.has(key))return;executedInitClips.add(key);fn(root);},
        callFrame(target,spec){
          const ref=references.get(target);
          if(!ref||!display.alive(target)||display.descriptor(target).owner!=='movie_clip'||spec===null||spec===undefined)return undefined;
          let normalized;
          if(typeof spec==='number'){if(!Number.isFinite(spec))return undefined;normalized=Math.trunc(spec);}
          else normalized=String(spec);
          if(callFrameDepth>=64)throw new RangeError('AS2 call() recursion limit exceeded.');
          const packet=JSON.parse(player.as2_call_frame_scripts(ref.key,JSON.stringify(normalized)));
          if(!packet||!packet.scripts?.length)return undefined;
          const scope=proxyForRef(packet.target_ref);
          callFrameDepth++;
          try{
            for(const script of packet.scripts){
              if(!script.source.trim())continue;
              const compiled=functions.get(sourceKey(script));
              if(!compiled)throw new Error('A script is missing from the compiled export. Re-export the project.');
              const event=Object.freeze({type:'frame',target:scope,currentTarget:scope});
              const scriptRoot=script.language==='ActionScript2'?as2RootFor(scope):proxyFor([]);
              compiled.fn.call(scope,event,scriptRoot,scope._parent,trace,scope.play,scope.stop,scope.gotoAndPlay,scope.gotoAndStop,scope.nextFrame,scope.prevFrame,as2.method,as2.value,program,display,legacyGlobals,xmlHost);
            }
          }finally{callFrameDepth--;}
          return undefined;
        },
        isFrameLoaded(target,args=[]){
          const ref=references.get(target);
          if(!ref||!display.alive(target)||display.descriptor(target).owner!=='movie_clip')return false;
          const hasScene=args.length>=2,scene=hasScene?(args[0]==null?null:String(args[0])):null,raw=hasScene?args[1]:args[0];
          let frame;
          if(typeof raw==='number'){if(!Number.isFinite(raw)||!Number.isInteger(raw))return false;frame=raw;}
          else if(typeof raw==='string')frame=raw;
          else return false;
          return Boolean(player.as2_is_frame_loaded(ref.key,JSON.stringify({scene,frame})));
        },
        globalPropRef:name=>{const ref=display.as2GlobalPropRef(name);if(!ref)throw new ReferenceError(`Unknown AS2 global property ${name}.`);return ref;},
        toggleHighQuality:display.toggleHighQuality,
        showRedrawRegions:display.showRedrawRegions,
        registerClass(id,ctor){
          if(typeof id!=='string'||!id.length||id.length>1024)return false;
          if(ctor===null||ctor===undefined){registeredClasses.set(id,null);return true;}
          if(typeof ctor!=='function')return false;
          const name=program?.className?.(ctor);
          if(name){
            if(program.dialect?.(name)!=='as2')throw new TypeError(`Object.registerClass requires an AS2 class or constructor function; ${name} is not an AS2 class.`);
            registeredClasses.set(id,{kind:'project',name,ctor});
            return true;
          }
          registeredClasses.set(id,{kind:'function',ctor});
          return true;
        },
        duplicateMovieClip:(target,name,depth)=>legacyDisplayCall(target,'duplicateMovieClip',[name,depth]),
        removeMovieClip:target=>legacyDisplayCall(target,'removeMovieClip',[]),
        startDrag:(target,...args)=>legacyDisplayCall(target,'startDrag',args),
        stopDrag:()=>legacyDisplayCall(proxyFor([]),'stopDrag',[]),
        targetPath:target=>display.targetPath(display.resolveTarget(target,proxyFor([]))),
        getProperty:(target,index)=>{const {receiver,name,native}=legacyProperty(target,index);return native?native.get():receiver[name];},
        setProperty:(target,index,value)=>{const {receiver,name,native}=legacyProperty(target,index);if(native)native.set(value);else receiver[name]=value;}});
      if(global.NeoFlashTextInput&&textHost&&stageHost&&typeof player.object_text_input==='function'){
        textInputHost=global.NeoFlashTextInput.create({getSize:logicalSize,player,text:textHost,display,events:global.NeoFlashEvents,hub:eventHub,stage:stageHost,referenceOf:target=>references.get(target),proxyForRef,onLegacyTab:event=>display.as2Tab?.(event)||false,beforeLegacyTextEvent:(target,name)=>{if(name==='onChanged')as2ObjectHost?.textChanged(target);},onLegacyTextEvent:(target,name,args)=>as2TextHost?.textEvent(references.get(target).key,name,args),
          canvas:typeof canvas==='undefined'?null:canvas,surface:typeof document==='undefined'?null:document.getElementById('stage-surface'),document:typeof document==='undefined'?null:document,project:typeof project==='undefined'?null:project,
          onError:inputFailure,afterInput:afterTimerEvent,onClick:(target,event)=>{target=display.resolveInput(target);if(!target)return;stageHost.handlePointer('click',event,target);if(display.alive(target)){player.object_click(references.get(target).key);dispatchScripts(target);if(typeof context!=='undefined')render();}}});
        legacyGlobals=Object.freeze({...legacyGlobals,Selection:Object.freeze({...textInputHost.Selection,setFocus:value=>{const target=typeof value==='string'?(value?display.resolveTarget(value,proxyFor([])):null):value;if(!display.as2CanFocus?.(target))return false;return textInputHost.Selection.setFocus(target);}})});
      }
      if(global.NeoFlashTimers&&global.NeoFlashEvents)timerHost=global.NeoFlashTimers.create({wasm,events:global.NeoFlashEvents,
        onError:timerFailure,afterEvent:afterTimerEvent,requestRender:requestTimerRender});
      if(timerHost?.legacy)legacyGlobals=Object.freeze({...legacyGlobals,...timerHost.legacy});
      if(global.NeoFlashEasing)easingHost=global.NeoFlashEasing.create(wasm,{timers:timerHost,events:global.NeoFlashEvents,getFrameRate:()=>runtimeFrameRate,onError:timerFailure,afterEvent:afterTimerEvent});
      if(global.NeoFlashBroadcaster){broadcasterHost=global.NeoFlashBroadcaster.create(wasm);legacyGlobals=Object.freeze({...legacyGlobals,AsBroadcaster:broadcasterHost.AsBroadcaster});}
      if(global.NeoFlashByteArray)binaryHost=global.NeoFlashByteArray.create({wasm});
      if(global.NeoFlashAMF&&binaryHost&&typeof player.amf_encode==='function'){
        amfHost=global.NeoFlashAMF.create({binary:binaryHost,encode:(format,graph)=>player.amf_encode(format,graph),decode:(format,bytes)=>player.amf_decode(format,bytes),getProgram:()=>program,isUnsupported:value=>references.has(value)});
        binaryHost.setObjectCodec(amfHost);
        if(global.NeoFlashStorage&&global.NeoFlashEvents){
          storageHost=global.NeoFlashStorage.create({amf:amfHost,events:global.NeoFlashEvents,onError:error=>console.warn(error)});
          legacyGlobals=Object.freeze({...legacyGlobals,SharedObject:storageHost.LegacySharedObject});
        }
      }
      if(global.NeoFlashExternal){
        externalHost=global.NeoFlashExternal.create({hostObject:typeof canvas==='undefined'?null:canvas,hostGlobal:global,getProgram:()=>program,isUnsupported:value=>references.has(value),afterCallback:afterTimerEvent,unwrapError:unwrapScriptThrow,onDiagnostic:error=>console.warn?.('NeoFlash ExternalInterface:',error)});
        legacyGlobals=Object.freeze({...legacyGlobals,ExternalInterface:externalHost.LegacyExternalInterface});
      }
      if(global.NeoFlashNet&&binaryHost&&global.NeoFlashEvents){
        netHost=global.NeoFlashNet.create({events:{...global.NeoFlashEvents,...eventFamilies},binary:binaryHost,resources:typeof project==='undefined'?[]:project.resources||[],onError:netFailure,onAfterDispatch:afterTimerEvent,navigate:prepared=>{if(!externalHost)throw new Error('Browser navigation adapter is unavailable. Re-export this publication.');externalHost.navigate(prepared);},legacyVariables:legacyNavigationVariables});
        legacyGlobals=Object.freeze({...legacyGlobals,LoadVars:netHost.LoadVars,getURL:function(url,target,method){return netHost.getURL(this,url,target,method);}});
      }
      if(global.NeoFlashAS2Network&&netHost){
        as2NetworkHost=global.NeoFlashAS2Network.create({wasm,getNet:()=>netHost,resolveTarget:(target,source)=>{const receiver=display.resolveTarget(target,references.has(source)?source:proxyFor([]));if(display.descriptor(receiver).owner!=='movie_clip')throw new TypeError('AS2 loadVariables target must be a live MovieClip.');return receiver;},fieldsOf:legacyNavigationVariables,alive:value=>display.alive(value),afterCallback:afterTimerEvent});
        legacyGlobals=Object.freeze({...legacyGlobals,loadVariables:function(url,target,method){return as2NetworkHost.load(this,url,target,method);},loadVariablesNum:function(url,level,method){if(level!==0)throw new RangeError('HTML5 loadVariablesNum supports level 0 only; additional SWF levels are unavailable.');return as2NetworkHost.load(this,url,proxyFor([]),method);}});
      }
      if(global.NeoFlashAS2Files&&netHost&&binaryHost&&broadcasterHost)as2FilesHost=global.NeoFlashAS2Files.create({wasm,net:netHost,binary:binaryHost,AsBroadcaster:broadcasterHost.AsBroadcaster,afterEvent:afterTimerEvent,onError:netFailure});
      if(global.NeoFlashAS2Locale&&netHost)as2LocaleHost=global.NeoFlashAS2Locale.create(wasm,{URLLoader:netHost.URLLoader,URLRequest:netHost.URLRequest,language:()=>as2PlatformHost.System.capabilities.language,afterEvent:afterTimerEvent,fields:{isText:value=>references.has(value)&&display.alive(value)&&display.descriptor(value).owner==='text'&&!as2TextHost.as2Ref(references.get(value).key,'html').get(),alive:value=>display.alive(value),write:(value,next)=>{if(as2TextHost.as2Ref(references.get(value).key,'html').get())throw new Error('Locale automatic replacement currently requires plain text.');textHost.update(references.get(value).key,{text:next});as2ObjectHost?.textChanged(value);}}});
      if(global.NeoFlashLoader&&netHost&&binaryHost&&global.NeoFlashEvents){
        loaderHost=global.NeoFlashLoader.create({player,events:global.NeoFlashEvents,net:netHost,binary:binaryHost,legacyVariables:legacyNavigationVariables,display,proxyForRef,referenceOf:target=>references.get(target),onMutation:deliverDisplayEvents,onError:netFailure,afterEvent:afterTimerEvent});
        legacyGlobals=Object.freeze({...legacyGlobals,MovieClipLoader:loaderHost.MovieClipLoader});
      }
      if(global.NeoFlashAudio&&netHost&&binaryHost&&global.NeoFlashEvents){
        audioHost=global.NeoFlashAudio.create({player,wasm,events:global.NeoFlashEvents,net:netHost,binary:binaryHost,resources:typeof project==='undefined'?[]:project.resources||[],onError:audioFailure,afterEvent:afterTimerEvent,onState:updateAudioStatus});
        legacyGlobals=Object.freeze({...legacyGlobals,Sound:audioHost.LegacySound,stopAllSounds:stopAllSounds});
      }
      if(global.NeoFlashAS2Connections&&netHost&&amfHost&&binaryHost){as2ConnectionsHost=global.NeoFlashAS2Connections.create({wasm,amf:amfHost,net:netHost,binary:binaryHost,onError:netFailure,afterEvent:afterTimerEvent});legacyGlobals=Object.freeze({...legacyGlobals,LocalConnection:as2ConnectionsHost.LocalConnection});}
      if(global.NeoFlashAS2Print&&bitmapHost){as2PrintHost=global.NeoFlashAS2Print.create(wasm,{
        resolveTarget:value=>{const target=value===0?proxyFor([]):typeof value==='string'?display.resolveTarget(value,proxyFor([])):value;if(!references.has(target)||!display.alive(target)||display.descriptor(target).owner!=='movie_clip')throw new TypeError('PrintJob requires level 0 or a live MovieClip.');return target;},
        bounds:target=>{if(target===proxyFor([])){const [width,height]=logicalSize();return {xMin:0,yMin:0,xMax:width,yMax:height};}const b=target.getBounds(target);return {xMin:b.x,yMin:b.y,xMax:b.x+b.width,yMax:b.y+b.height};},
        capture:(target,plan)=>{const bitmap=new bitmapHost.BitmapData(plan.width,plan.height,false,0xffffff);try{bitmap.draw(target,new geometryHost.Matrix(1,0,0,1,-plan.x,-plan.y));return player.bitmap_encode_png(JSON.stringify(bitmapHost.refOf(bitmap)),0,0,plan.width,plan.height,false);}finally{bitmap.dispose();}},onError:netFailure});legacyGlobals=Object.freeze({...legacyGlobals,PrintJob:as2PrintHost.PrintJob});}
      if(global.NeoFlashAS2Capture){as2CaptureHost=global.NeoFlashAS2Capture.create(wasm,{referenceOf:target=>references.get(target),display,getAudio:()=>audioHost,afterEvent:afterTimerEvent,onError:netFailure});legacyGlobals=Object.freeze({...legacyGlobals,Camera:as2CaptureHost.Camera,Microphone:as2CaptureHost.Microphone});}
      if(global.NeoFlashVideo&&netHost&&global.NeoFlashEvents&&typeof player.object_video_info==='function'){
        videoHost=global.NeoFlashVideo.create({getCapture:()=>as2CaptureHost,getRemoting:()=>as2ConnectionsHost?.remoting,player,wasm,events:global.NeoFlashEvents,net:netHost,getSoundTransform:()=>audioHost?.SoundTransform,referenceOf:target=>references.get(target),proxyForRef,onError:netFailure,afterEvent:afterTimerEvent,onFrame:()=>{if(typeof context!=='undefined')render();},onState:updateAudioStatus});
        const LegacyVideo=function(){throw new TypeError('AS2 Video must be an authored Video placement; new Video() is not available.');};
        Object.defineProperties(LegacyVideo,{__nfIs:{value:videoHost.Video.__nfIs},[Symbol.hasInstance]:{value:videoHost.Video.__nfIs}});
        legacyGlobals=Object.freeze({...legacyGlobals,Video:LegacyVideo,NetConnection:videoHost.LegacyNetConnection,NetStream:videoHost.LegacyNetStream});
      }
      if(global.NeoFlashAS2DepthManager&&typeof player.as2_depth_manager==='function')as2DepthManagerHost=global.NeoFlashAS2DepthManager.create({player,referenceOf:target=>references.get(target),display,getRoot:()=>proxyFor([]),afterEvent:afterTimerEvent});
      if(global.NeoFlashAS2Transitions&&easingHost)as2TransitionsHost=global.NeoFlashAS2Transitions.create({wasm,timers:timerHost,easing:easingHost,EventDispatcher:as2PlatformHost.EventDispatcher,stage:stageHost.stage,getFrameRate:()=>runtimeFrameRate,isMovieClip:target=>display.alive(target)&&display.descriptor(target).owner==='movie_clip',afterEvent:afterTimerEvent,onError:timerFailure});
      if(global.NeoFlashAS2Media&&videoHost)as2MediaHost=global.NeoFlashAS2Media.create(wasm,{video:videoHost,EventDispatcher:as2PlatformHost.EventDispatcher,afterEvent:afterTimerEvent});
      if(global.NeoFlashXML){
        xmlHost=global.NeoFlashXML.create({parseLegacy:source=>wasm.parse_as2_xml_json(source),namespaceLookup:(ancestors,value,reverse)=>JSON.parse(wasm.as2_xml_namespace_json(JSON.stringify(ancestors),value,reverse)),parse:source=>{const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;if(typeof wasm?.parse_xml_json!=='function')throw new Error('XML parser is missing. Re-export this project.');return wasm.parse_xml_json(source);},net:netHost,onError:netFailure});
        legacyGlobals=Object.freeze({...legacyGlobals,XML:xmlHost.LegacyXML,XMLNode:xmlHost.XMLNode});
      }
      if(textHost)legacyGlobals=Object.freeze({...legacyGlobals,TextFormat:textHost.As2TextFormat||textHost.TextFormat});
      if(as2TextHost){
        const LegacyTextField=function(){throw new TypeError('Create an AS2 TextField with MovieClip.createTextField.');};
        LegacyTextField.StyleSheet=as2TextHost.StyleSheet;
        LegacyTextField.getFontList=as2TextHost.getFontList;
        legacyGlobals=Object.freeze({...legacyGlobals,TextField:LegacyTextField,nativeRef:(receiver,name)=>{
          if(name==='getFontList'&&(receiver===LegacyTextField||receiver===display.builtins['flash.text.TextField']))return {has:()=>true,delete:()=>false,get:()=>as2TextHost.getFontList,set:()=>{throw new TypeError('TextField.getFontList is read-only.');}};
          const menu=as2StageHost?.menuRef(receiver,name);if(menu)return menu;
          const geometry=geometryHost.as2Ref?.(receiver,name);if(geometry)return geometry;
          const ref=references.get(receiver);if(!ref)return null;const input=display.as2Ref?.(receiver,name);if(input)return input;
          if(as2DepthManagerHost?.methodNames.includes(name)&&display.descriptor(receiver).owner==='movie_clip')return {has:()=>true,delete:()=>false,get:()=>as2DepthManagerHost.methods(receiver)[name],set:()=>{throw new TypeError('DepthManager MovieClip methods are read-only.');}};
          if(name==='attachAudio'&&as2CaptureHost&&display.descriptor(receiver).owner==='movie_clip')return {get:()=>value=>as2CaptureHost.attachAudio(receiver,value),has:()=>true,delete:()=>false,set:()=>{throw new TypeError('MovieClip.attachAudio is read-only.');}};
          if(name==='attachVideo'&&videoHost&&display.descriptor(receiver).owner==='video')return {get:()=>value=>videoHost.legacyAttachVideo(receiver,value),has:()=>true,delete:()=>false,set:()=>{throw new TypeError('Video.attachVideo is read-only.');}};
          if(name==='loadVariables'&&as2NetworkHost&&display.descriptor(receiver).owner==='movie_clip')return {get:()=>((url,method)=>as2NetworkHost.load(receiver,url,receiver,method)),has:()=>true,delete:()=>false,set:()=>{throw new TypeError('MovieClip.loadVariables is read-only.');}};if(display.descriptor(receiver).owner!=='text')return null;
          if(name==='variable')return as2ObjectHost?.variableRef(receiver)||null;
          const property=as2TextHost.as2Ref(ref.key,name);
          if(name==='text'&&property)return {...property,get:()=>{as2ObjectHost?.syncTextVariables();return property.get();},set:value=>{const result=property.set(value);as2ObjectHost?.textChanged(receiver);return result;}};
          return property;
        }});
      }
      if(global.NeoFlashAS2Object){as2ObjectHost=global.NeoFlashAS2Object.create(wasm,{textVariable:{isText:value=>references.has(value)&&display.alive(value)&&display.descriptor(value).owner==='text',isMovieClip:value=>references.has(value)&&display.alive(value)&&display.descriptor(value).owner==='movie_clip',alive:value=>display.alive(value),parentOf:value=>value._parent,readText:value=>textHost.get(references.get(value).key,'text'),writeText:(value,next)=>textHost.update(references.get(value).key,{text:next}),isHTML:value=>as2TextHost.as2Ref(references.get(value).key,'html').get()},nativeRef:(receiver,name)=>legacyGlobals.nativeRef?.(receiver,name),globals:frame=>({_root:as2RootFor(frame),_level0:proxyFor([]),_global:legacyGlobals._global}),globalProp:name=>display.as2GlobalPropRef(name),isNative:value=>references.has(value),isProject:value=>program?.enumerationKeys?.(value)!==null&&program?.enumerationKeys?.(value)!==undefined});legacyGlobals=Object.freeze({...legacyGlobals,objects:as2ObjectHost});}
      if(global.NeoFlashAS2Binding)as2BindingHost=global.NeoFlashAS2Binding.create(wasm,{EventDispatcher:as2PlatformHost.EventDispatcher});
      if(global.NeoFlashAS2Data)as2DataHost=global.NeoFlashAS2Data.create(wasm,{EventDispatcher:as2PlatformHost.EventDispatcher});
      if(global.NeoFlashAS2Services&&netHost&&xmlHost)as2ServicesHost=global.NeoFlashAS2Services.create({wasm,net:netHost,LegacyXML:xmlHost.LegacyXML,EventDispatcher:as2PlatformHost.EventDispatcher,ComponentMixins:as2BindingHost?.builtins["mx.data.binding.ComponentMixins"],onError:netFailure,afterEvent:afterTimerEvent});
      as2.setResolver((receiver,name)=>{const ref=legacyGlobals.nativeRef?.(receiver,name);return ref?ref.get():as2ObjectHost?as2ObjectHost.get(receiver,name):receiver[name];});
      if(global.NeoFlashAS2Strings){
        const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;
        legacyGlobals=Object.freeze({...legacyGlobals,strings:global.NeoFlashAS2Strings.create({execute:request=>wasm.as2_string_json(request)})});
      }
      if(global.NeoFlashAS2Boxing&&legacyGlobals.strings){as2BoxingHost=global.NeoFlashAS2Boxing.create({execute:request=>wasm.as2_box_json(request),strings:legacyGlobals.strings});legacyGlobals=Object.freeze({...legacyGlobals,boxing:as2BoxingHost});}
      if(global.NeoFlashAS3Strings)legacyGlobals=Object.freeze({...legacyGlobals,strings3:global.NeoFlashAS3Strings.create({execute:request=>wasm.as3_string_json(request)})});
      if(global.NeoFlashMath)legacyGlobals=Object.freeze({...legacyGlobals,math:global.NeoFlashMath.create(wasm)});
      legacyGlobals=Object.freeze({...legacyGlobals,objects3:Object.freeze({
        execute:request=>JSON.parse(wasm.as3_object_json(JSON.stringify(request))),
        arrayPlan:(method,length,index)=>JSON.parse(wasm.as3_array_plan_json(method,length,index)),
        arrays:{classes:new Map(),methods:new WeakMap()},
      })});
      if(global.NeoFlashDate)legacyGlobals=Object.freeze({...legacyGlobals,date:global.NeoFlashDate.create(wasm)});
      if(geometryHost.Color)legacyGlobals=Object.freeze({...legacyGlobals,Color:geometryHost.Color});
      initializeProgram();
    }
    function initializeProgram() {
      if(!display)return;
      const builtins={...display.builtins};
      // FrameLabel stores a Rust value and uses the existing event dispatcher.
      // Automatic frameLabel delivery is separate from this readback profile.
      if(global.NeoFlashEvents){
        const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;
        const values=new WeakMap();
        frameLabelType=class FrameLabel extends global.NeoFlashEvents.EventDispatcher {
          constructor(name,frame){
            if(arguments.length!==2||typeof name!=='string'||!Number.isInteger(frame)||frame < -2147483648||frame > 2147483647)throw new TypeError('FrameLabel requires a primitive String name and a signed 32-bit integer frame.');
            if(typeof wasm?.frame_label_json!=='function')throw new Error('Rust FrameLabel values are unavailable; rebuild and re-export this project.');
            let value;try{value=JSON.parse(wasm.frame_label_json(name,frame));}catch(error){throw new TypeError(String(error?.message??error));}
            super();values.set(this,value);
            const add=this.addEventListener;
            this.addEventListener=(type,...args)=>{if(type==='frameLabel')throw new TypeError('Automatic FrameLabel events are not implemented; read MovieClip.currentFrameLabel when handling timeline input.');return add(type,...args);};
            Object.freeze(this);
          }
          get name(){const value=values.get(this);if(!value)throw new TypeError('Invalid FrameLabel receiver.');return value.name;}
          get frame(){const value=values.get(this);if(!value)throw new TypeError('Invalid FrameLabel receiver.');return value.frame;}
        };
        frameLabelType.__nfIs=value=>values.has(value);
        frameLabelType.__nfInitialize=()=>{throw new TypeError('FrameLabel is final and cannot initialize a subclass.');};
        builtins['flash.display.FrameLabel']=frameLabelType;
        const sceneValues=new WeakMap(),sceneToken={};
        class Scene {
          constructor(token,value){
            if(token!==sceneToken)throw new TypeError('Scene values are produced by MovieClip.currentScene/scenes; public Scene construction is not supported.');
            sceneValues.set(this,value);Object.freeze(this);
          }
          get name(){const value=sceneValues.get(this);if(!value)throw new TypeError('Invalid Scene receiver.');return value.name;}
          get labels(){const value=sceneValues.get(this);if(!value)throw new TypeError('Invalid Scene receiver.');return value.labels;}
          get numFrames(){const value=sceneValues.get(this);if(!value)throw new TypeError('Invalid Scene receiver.');return value.numFrames;}
        }
        Scene.__nfIs=value=>sceneValues.has(value);
        Scene.__nfInitialize=()=>{throw new TypeError('Scene is final and cannot initialize a subclass.');};
        sceneValueFactory=value=>new Scene(sceneToken,{name:value.name,numFrames:value.num_frames,labels:value.labels.map(label=>new frameLabelType(label.name,label.frame))});
        builtins['flash.display.Scene']=Scene;
      }
      builtins.__nfTrace=trace;
      if(as2StageHost){builtins.ContextMenu=as2StageHost.ContextMenu;builtins.ContextMenuItem=as2StageHost.ContextMenuItem;}
      if(as2PrintHost)builtins.PrintJob=as2PrintHost.PrintJob;
      if(as2CaptureHost){builtins.Camera=as2CaptureHost.Camera;builtins.Microphone=as2CaptureHost.Microphone;}
      if(as2ConnectionsHost)builtins.LocalConnection=as2ConnectionsHost.LocalConnection;
      if(as2DataHost)Object.assign(builtins,as2DataHost.builtins);
      if(as2DepthManagerHost)builtins['mx.managers.DepthManager']=as2DepthManagerHost.DepthManager;
      if(as2TransitionsHost)Object.assign(builtins,as2TransitionsHost.builtins);
      if(as2MediaHost)Object.assign(builtins,as2MediaHost.builtins);
      if(as2ServicesHost)Object.assign(builtins,as2ServicesHost.builtins);
      if(as2BindingHost)Object.assign(builtins,as2BindingHost.builtins);
      if(as2LocaleHost)Object.assign(builtins,as2LocaleHost.builtins);
      if(as2FilesHost){builtins['flash.net.FileReference']=as2FilesHost.FileReference;builtins['flash.net.FileReferenceList']=as2FilesHost.FileReferenceList;}
      if(as2TextHost){builtins['TextField.StyleSheet']=as2TextHost.StyleSheet;builtins['flash.text.TextRenderer']=as2TextHost.TextRenderer;builtins.TextSnapshot=as2TextHost.TextSnapshot;}
      if(filterHost?.DisplacementMapFilter)builtins['flash.filters.DisplacementMapFilter']=filterHost.DisplacementMapFilter;
      if(as2PlatformHost){builtins['mx.utils.Delegate']=as2PlatformHost.Delegate;builtins['mx.events.EventDispatcher']=as2PlatformHost.EventDispatcher;}
      builtins.__nfAs2Method=as2.method;builtins.__nfAs2Value=as2.value;
      builtins.__nfDisplay=display;builtins.__nfLegacy=legacyGlobals;
      builtins.__nfObjectExecute=legacyGlobals.objects3.execute;
      if(global.NeoFlashGeom3D){
        const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;
        geometry3dHost=global.NeoFlashGeom3D.create(wasm,{getPoint:()=>geometryHost.Point});
        for(const name of ['Vector3D','Matrix3D','Utils3D','PerspectiveProjection'])builtins['flash.geom.'+name]=geometry3dHost[name];
        builtins.__nfGeometryNumberVector=geometry3dHost.setNumberVectorFactory;
      }
      if(geometryHost.MotionColor)builtins['fl.motion.Color']=geometryHost.MotionColor;
      if(geometryHost.MatrixTransformer)builtins['fl.motion.MatrixTransformer']=geometryHost.MatrixTransformer;
      if(easingHost)Object.assign(builtins,easingHost.builtins);
      if(global.NeoFlashNumberFormat){
        const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;
        const numberFormat=global.NeoFlashNumberFormat.create(wasm);
        for(const name of ['NumberFormatter','NumberParseResult'])builtins['flash.globalization.'+name]=numberFormat[name];
      }
      if(legacyGlobals?.date){
        for(const name of ['DateTimeFormatter','DateTimeStyle','DateTimeNameStyle','DateTimeNameContext'])builtins['flash.globalization.'+name]=legacyGlobals.date[name];
        builtins.__nfDateStringVector=legacyGlobals.date.setStringVectorFactory;
      }
      if(legacyGlobals?.strings3)for(const name of ['escapeMultiByte','unescapeMultiByte'])builtins['flash.utils.'+name]=legacyGlobals.strings3[name];
      builtins.__nfHasNativeProperty=(receiver,name)=>Boolean(references.get(receiver)?.nativeHas(name));
      builtins.__nfIsNativeDisplay=receiver=>references.has(receiver);
      if(xmlHost){builtins.__nfXML=xmlHost;builtins.XML=xmlHost.XML;builtins.XMLList=xmlHost.XMLList;}
      if(global.NeoFlashEvents)for(const name of ['Event','MouseEvent','KeyboardEvent','FocusEvent','TextEvent','EventDispatcher','EventPhase'])builtins['flash.events.'+name]=global.NeoFlashEvents[name];
      for(const [name,type] of Object.entries(eventFamilies))builtins['flash.events.'+name]=type;
      if(global.NeoFlashStage)builtins['flash.display.Stage']=global.NeoFlashStage.Stage;
      if(timerHost){builtins['flash.utils.Timer']=timerHost.Timer;builtins['flash.events.TimerEvent']=timerHost.TimerEvent;builtins['flash.utils.getTimer']=timerHost.getTimer;}
      if(timerHost)for(const name of ['setInterval','setTimeout','clearInterval','clearTimeout'])builtins['flash.utils.'+name]=timerHost[name];
      if(bitmapHost){builtins['flash.display.BitmapData']=bitmapHost.BitmapData;builtins['flash.display.PixelSnapping']=bitmapHost.PixelSnapping;builtins['flash.display.PNGEncoderOptions']=bitmapHost.PNGEncoderOptions;}
      if(textHost){builtins['flash.text.TextFormat']=textHost.TextFormat;builtins['flash.text.TextFormatAlign']=textHost.TextFormatAlign;builtins['flash.text.TextFieldType']=textHost.TextFieldType;}
      if(filterHost)for(const name of ['BitmapFilter','BlurFilter','GlowFilter','DropShadowFilter','ColorMatrixFilter','BitmapFilterQuality','ConvolutionFilter','BevelFilter','GradientGlowFilter','GradientBevelFilter','BitmapFilterType'])builtins['flash.filters.'+name]=filterHost[name];
      if(storageHost)for(const name of ['SharedObject','SharedObjectFlushStatus','ObjectEncoding'])builtins['flash.net.'+name]=storageHost[name];
      if(loaderHost)for(const name of ['Loader','LoaderInfo'])builtins['flash.display.'+name]=loaderHost[name];
      if(videoHost){builtins['flash.media.Video']=videoHost.Video;builtins['flash.events.NetStatusEvent']=videoHost.NetStatusEvent;for(const name of ['NetConnection','NetStream'])builtins['flash.net.'+name]=videoHost[name];}
      if(audioHost){for(const name of ['Sound','SoundChannel','SoundTransform'])builtins['flash.media.'+name]=audioHost[name];builtins['flash.media.SoundMixer']=SoundMixer;}
      if(binaryHost){builtins['flash.utils.ByteArray']=binaryHost.ByteArray;builtins['flash.utils.Endian']=binaryHost.Endian;}
      if(externalHost)builtins['flash.external.ExternalInterface']=externalHost.ExternalInterface;
      if(netHost){
        for(const name of ['URLRequest','URLRequestHeader','URLRequestMethod','URLVariables','URLLoader','URLLoaderDataFormat'])builtins['flash.net.'+name]=netHost[name];
        for(const name of ['navigateToURL','sendToURL'])builtins['flash.net.'+name]=netHost[name];
        for(const name of ['ProgressEvent','IOErrorEvent','HTTPStatusEvent'])builtins['flash.events.'+name]=netHost[name];
      }
      program=typeof bundle!=='undefined'&&bundle.program ? new Function('__nfBuiltins',bundle.program.code)(builtins) : {
        type:name=>{if(!builtins[name])throw new TypeError(`Unsupported built-in class ${name}.`);return builtins[name];},
        is:(value,name)=>Boolean(builtins[name]?.__nfIs?.(value)),
      };
    }
    function resetProject() {
      pointerOverContent=false;legacyCursorVisible=true;updateCursor();
      capturePainter?.clear();capturePainter=null;
      filterHost?.renderer.clear();
      as2PrintHost?.reset();as2BoxingHost?.reset();as2StageHost?.reset();as2ConnectionsHost?.reset();as2DepthManagerHost?.reset();as2TransitionsHost?.reset();as2MediaHost?.reset();as2ServicesHost?.reset();as2FilesHost?.reset();as2NetworkHost?.reset();as2LocaleHost?.reset();as2TextHost?.reset();as2BindingHost?.reset();as2DataHost?.reset();as2ObjectHost?.reset();
      externalHost?.reset();textInputHost?.reset();storageHost?.reset();loaderHost?.reset();xmlHost?.reset();videoHost?.reset();as2CaptureHost?.reset();audioHost?.reset();netHost?.reset();stopTimers();binaryHost?.reset();bitmapHost?.reset();textHost?.reset();
      proxies.clear(); attachedClasses.clear(); registeredClasses.clear(); executedInitClips.clear(); callFrameDepth = 0; if(legacyGlobals?.__nfCallStack)legacyGlobals.__nfCallStack.length=0; display?.clear();eventHub?.clear();stageHost?.reset();legacyHost?.reset();as2PlatformHost?.reset();for(const key of Object.keys(legacyGlobals?._global||{}))delete legacyGlobals._global[key];lastTrace = '';
      soundsPreloaded=false;
      player.reset();
      initializeProgram();
      dispatchScripts();
    }
    function disposeFamily() {
      pointerOverContent=false;legacyCursorVisible=true;updateCursor();
      capturePainter?.clear();capturePainter=null;
      filterHost?.renderer.clear();
      as2PrintHost?.dispose();as2BoxingHost?.dispose();as2StageHost?.dispose();as2ConnectionsHost?.dispose();as2DepthManagerHost?.dispose();as2TransitionsHost?.dispose();as2MediaHost?.dispose();as2ServicesHost?.dispose();as2FilesHost?.dispose();as2NetworkHost?.dispose();as2LocaleHost?.dispose();as2TextHost?.dispose();as2BindingHost?.dispose();as2DataHost?.dispose();as2ObjectHost?.dispose();
      externalHost?.dispose();textInputHost?.dispose();storageHost?.dispose();loaderHost?.dispose();xmlHost?.dispose();videoHost?.dispose();as2CaptureHost?.dispose();audioHost?.dispose();netHost?.dispose();timerRenderGeneration++;easingHost?.dispose?.();broadcasterHost?.dispose?.();timerHost?.dispose();binaryHost?.dispose();bitmapHost?.dispose();textHost?.dispose();
      registeredClasses.clear();executedInitClips.clear();callFrameDepth=0;if(legacyGlobals?.__nfCallStack)legacyGlobals.__nfCallStack.length=0;
      as2PlatformHost?.dispose();legacyHost?.dispose();stageHost?.dispose();eventHub?.clear();display?.clear();
    }
    function as2RootFor(target) {
      const ref=references.get(target);
      return graphMode&&ref?.objectRef&&typeof player.as2_root==='function'?proxyForRef(JSON.parse(player.as2_root(ref.key))):proxyFor([]);
    }
    function prepareLinked(receiver,allocatedByProgram,linkageId=null) {
      const hasRegistered=linkageId!==null&&registeredClasses.has(linkageId);
      const registered=hasRegistered?registeredClasses.get(linkageId):undefined;
      if(!hasRegistered&&!program?.attach&&display.descriptor(receiver).class_name)throw new Error('Linked symbol class construction requires an initialized project program; creation during static field initialization is not supported yet.');
      if(allocatedByProgram||hasRegistered){const ref=references.get(receiver);attachedClasses.set(ref.key,ref.lifetime);}
      attachNewInstances(receiver);
      if(hasRegistered&&registered!==null){
        if(registered.kind==='project')program.attach(registered.name,receiver);
        else{
          if(registered.ctor.prototype&&typeof registered.ctor.prototype==='object')Object.setPrototypeOf(receiver,registered.ctor.prototype);
          Object.defineProperty(receiver,'__constructor__',{configurable:true,writable:true,enumerable:false,value:registered.ctor});
          Reflect.apply(registered.ctor,receiver,[]);
        }
        display.refreshLegacyCallbacks(receiver,true);
      }
    }
    function attachNewInstances(scope=null) {
      if(!display)return;
      display.prune();eventHub?.prune();
      const nodes=display.nodes(),byRef=new Map(nodes.map(node=>[JSON.stringify(node.object_ref),node]));
      const depth=node=>{let result=0;while(node?.parent_ref){node=byRef.get(JSON.stringify(node.parent_ref));result++;}return result;};
      const withinScope=node=>{
        if(!scope)return true;
        const key=references.get(scope).key;
        for(let current=node;current;current=byRef.get(JSON.stringify(current.parent_ref)))if(JSON.stringify(current.object_ref)===key)return true;
        return false;
      };
      // Initialize authored descendants in each namespace before their linked
      // container constructor. Native allocation reserves its own root first.
      for(const node of nodes.sort((a,b)=>graphMode?depth(b)-depth(a):b.path.length-a.path.length)) {
        if(graphMode&&!withinScope(node))continue;
        const symbolId=node.symbol_id||node.kind?.symbol_id;
        const symLinkage=symbolId&&typeof project!=='undefined'?project?.symbols?.find(s=>s.id===symbolId)?.linkage_id:null;
        const hasRegistered=Boolean(symLinkage&&registeredClasses.has(symLinkage));
        if(graphMode&&node.authored_path===null&&!node.class_name&&!hasRegistered)continue;
        const key=graphMode?JSON.stringify(node.object_ref):JSON.stringify(node.path),previous=attachedClasses.get(key);
        const lifetime=graphMode?node.object_ref.generation:node.lifetime_id;
        if(graphMode?!player.object_alive(key):player.lifetime_id(key)!==lifetime)continue;
        if(previous===lifetime)continue;
        attachedClasses.set(key,lifetime);
        const receiver=graphMode?proxyForRef(node.object_ref):proxyFor(node.path);
        const registered=hasRegistered?registeredClasses.get(symLinkage):undefined;
        if(hasRegistered){
          if(registered!==null){
            if(registered.kind==='project')program.attach(registered.name,receiver);
            else{
              if(registered.ctor.prototype&&typeof registered.ctor.prototype==='object')Object.setPrototypeOf(receiver,registered.ctor.prototype);
              Object.defineProperty(receiver,'__constructor__',{configurable:true,writable:true,enumerable:false,value:registered.ctor});
              Reflect.apply(registered.ctor,receiver,[]);
            }
            display.refreshLegacyCallbacks(receiver,true);
          }
        }else if(node.class_name){
          if(!program?.attach)throw new Error(`Linked class ${node.class_name} has no compiled project program.`);
          program.attach(node.class_name,receiver);
          if(program.dialect?.(node.class_name)==='as2')display.refreshLegacyCallbacks(receiver,true);
        }
      }
      if(scope)return;
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
        display?.lifecycle?.(item.kind,target);
      }
    }
    function drainDisplayEvents() {
      if(graphMode&&typeof player.display_events_json==='function')deliverDisplayEvents(JSON.parse(player.display_events_json()).events);
    }
    function semanticRoute(node,list){
      if(node.owner_ref&&!player.object_alive(JSON.stringify(node.owner_ref)))return null;
      const owner=node.owner_ref?proxyForRef(node.owner_ref):proxyFor(node.instance_path);
      const route=display?.inputRoute?display.inputRoute(owner,list):{owner,target:owner};
      return route?.owner===owner?route:null;
    }
    function activateSemantic(node) {
      const route=semanticRoute(node);if(!route)return false;
      const accepted=node.owner_ref&&typeof player.object_activate_accessible==='function'
        ?player.object_activate_accessible(JSON.stringify(node.owner_ref))
        :player.activate_accessible(JSON.stringify(node.instance_path),node.lifetime_id);
      if(!accepted)return false;
      const target=route.target;
      // Native semantic buttons have one click activation and no Canvas pointerdown.
      // Deliver the legacy press action once, alongside the AS3 click route.
      if(display){display.pointer('mouseDown',target);display.pointer('mouseUp',target);}
      if(stageHost&&display.alive(target)){stageHost.stage.focus=target;stageHost.handlePointer('click',{},target);}
      dispatchScripts(route.owner);if(typeof context!=='undefined')render();return true;
    }
    function interactiveTarget(hit) {
      let target=graphMode?proxyForRef(hit):proxyFor(hit);
      return display.resolveInput(target);
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
      const call=(name,...args)=>player[objectRef?'object_'+({clip_frame:'frame',clip_frame_count:'frame_count',clip_metadata:'metadata'}[name]||name):name](key,...args);
      const custom = Object.create(null);
      function assertAlive() {
        if (!live()) throw new Error(`Display object ${key} was removed or replaced.`);
      }
      function ownsTimelineMetadata(name){
        if(!Object.hasOwn(timelineMetadataProperties,name))return false;
        assertAlive();
        return !objectRef||display?.descriptor(proxy).owner==='movie_clip';
      }
      function timelineMetadata(name){
        assertAlive();
        let value;try{value=JSON.parse(call('clip_metadata'));}catch(error){throw new TypeError(String(error?.message??error));}
        if(name==='currentScene'||name==='scenes'){
          if(!sceneValueFactory)throw new Error('Scene values are unavailable; rebuild and re-export this project.');
          return name==='scenes'?value.scenes.map(sceneValueFactory):sceneValueFactory({name:value.scene_name,num_frames:value.total_frames,labels:value.labels});
        }
        const field=value[timelineMetadataProperties[name]];
        if(name!=='currentLabels')return field;
        if(!frameLabelType)throw new Error('FrameLabel type is unavailable; rebuild and re-export this project.');
        return field.map(label=>new frameLabelType(label.name,label.frame));
      }
      const target = Object.create(null);
      function dynamicGet(name){
        if(Object.hasOwn(target,name))return Reflect.get(target,name,proxy);
        if(Object.hasOwn(custom,name))return custom[name];
        if(Reflect.has(target,name))return Reflect.get(target,name,proxy);
        if(objectRef){const child=display.children(proxy).find(node=>node.name===name);return child?display.nodeProxy(child):undefined;}
        const child=JSON.parse(player.child_path(key,name));return child?proxyFor(child):undefined;
      }
      function dynamicHas(name){
        if(Reflect.has(target,name)||Object.hasOwn(custom,name))return true;
        if(typeof name!=='string')return false;
        if(objectRef)return display.children(proxy).some(node=>node.name===name);
        return JSON.parse(player.child_path(key,name))!==null;
      }
      function dynamicSet(name,value){
        if(Object.hasOwn(target,name)||(!Object.hasOwn(custom,name)&&Reflect.has(target,name)))return Reflect.set(target,name,value,proxy);
        program?.assertDynamicWrite?.(proxy,name);custom[name]=value;return true;
      }
      function dynamicDelete(name){
        if(program?.hasFixedProperty?.(proxy,name))return false;
        if(Object.hasOwn(custom,name))return delete custom[name];
        return Reflect.deleteProperty(target,name);
      }
      // These names are native AS3 traits but remain ordinary AS2 properties.
      // The per-access AS2 reference preserves mixed-dialect instance identity.
      function as2TimelineRef(name){
        if(!ownsTimelineMetadata(name))return null;
        return {
          get(){assertAlive();return dynamicGet(name);},
          has(){assertAlive();return dynamicHas(name);},
          set(value){assertAlive();if(!dynamicSet(name,value))throw new TypeError(`Cannot write AS2 property ${name}.`);return value;},
          delete(){assertAlive();return dynamicDelete(name);},
        };
      }
      const methods = {
        play: () => control('play'),
        stop: () => control('stop'),
        gotoAndPlay: (value, scene) => goto('goto_and_play', value, false, scene),
        gotoAndStop: (value, scene) => goto('goto_and_stop', value, false, scene),
        nextFrame: () => seek('goto_and_stop', call('clip_frame') + 1),
        prevFrame: () => seek('goto_and_stop', call('clip_frame') - 1),
        nextScene: () => sceneStep(1, 'goto_and_play'),
        prevScene: () => sceneStep(-1, 'goto_and_play'),
        trace: (...values) => trace(...values),
      };
      function control(command) {assertAlive();if(!call('control',command,0))throw new Error(`Cannot control timeline at ${key}.`);}
      // Only a multi-scene main timeline reports more than one scene.
      function sceneCount(){return typeof player[objectRef?'object_scene_count':'scene_count']==='function'?call('scene_count'):1;}
      function goto(command, value, legacy = false, scene = null) {
        assertAlive();
        const total = call('clip_frame_count');
        if (!total) throw new Error('Timeline controls apply to symbols and the root scene.');
        let frame;
        if (scene != null || sceneCount() > 1) {
          // Rust resolves scene names, scene-relative numbers and labels.
          if (typeof value !== 'number' && typeof value !== 'string') {
            throw new Error(legacy ? 'AS2 goto requires a frame label or integer frame; other argument coercions are not supported.' : 'Frame must be a frame number or a frame label.');
          }
          const request = JSON.stringify({frame: typeof value === 'number' && !Number.isFinite(value) ? String(value) : value, scene: scene == null ? null : String(scene), as2: legacy});
          try { frame = call('resolve_goto', request); } catch (error) { throw new Error(String(error?.message ?? error)); }
        } else {
          frame = legacy
            ? as2Frame(value, total, label => call('resolve_frame_as2',label))
            : oneBasedFrame(value, total, label => call('resolve_frame',label));
        }
        if (frame === null || frame < 0) return; // An unknown AS2 label changes neither frame nor play state.
        if (!call('control',command,frame)) throw new Error(`Cannot control timeline at ${key}.`);
      }
      // Zero-based seek used by nextFrame/prevFrame, which cross scene boundaries.
      function seek(command, frame) {
        assertAlive();
        const total = call('clip_frame_count');
        if (!total) throw new Error('Timeline controls apply to symbols and the root scene.');
        if (!call('control',command,Math.max(0, Math.min(frame, total - 1)))) throw new Error(`Cannot control timeline at ${key}.`);
      }
      function sceneStep(step, command) {
        assertAlive();
        if (sceneCount() <= 1) return; // No adjacent scene: the playhead stays.
        const frame = call('scene_step', step);
        if (frame >= 0) seek(command, frame);
      }
      function nativeHas(name){
        if(typeof name!=='string')return name===Symbol.toStringTag;
        if(ownsTimelineMetadata(name))return true;
        if(['toString','toJSON','root','_root','parent','_parent','currentFrame','_currentframe','totalFrames','_totalframes','visible','_visible'].includes(name)||Object.hasOwn(methods,name)||Object.hasOwn(propertyMap,name))return true;
        return Boolean(display&&display.get(proxy,name)!==display.UNHANDLED);
      }
      const proxy = new Proxy(target, {
        has(_target,name){
          return nativeHas(name)||dynamicHas(name);
        },
        deleteProperty(_target,name){
          return !nativeHas(name)&&dynamicDelete(name);
        },
        get(_target, name) {
          if (name === Symbol.toStringTag) return 'NeoFlashDisplayObject';
          if (name === 'toString') return () => `[NeoFlash ${key}]`;
          if (name === 'toJSON') return () => objectRef?({object_ref:objectRef}):({path});
          if (typeof name !== 'string') return undefined;
          if (name === '_root' || name === 'root') {assertAlive();return name==='root'&&objectRef&&!display.descriptor(proxy).on_stage?null:name==='_root'?as2RootFor(proxy):proxyFor([]);}
          if (name === '_parent' || name === 'parent') {assertAlive();const parent=display?display.parentSpace(proxy):path.length?proxyFor(path.slice(0,-1)):stageHost?.stage||null;return name==='_parent'&&parent===stageHost?.stage?null:parent;}
          if (name === 'currentFrame' && sceneCount() > 1) {assertAlive();return JSON.parse(call('clip_metadata')).scene_frame;} // AS3 frames are scene-relative.
          if (name === '_currentframe' || name === 'currentFrame') return call('clip_frame') + 1;
          if (name === '_totalframes' || name === 'totalFrames') return call('clip_frame_count');
          if (ownsTimelineMetadata(name)) return timelineMetadata(name);
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
          return dynamicGet(name);
        },
        set(_target, name, value) {
          if (typeof name !== 'string') return false;
          if(display&&display.set(proxy,name,value)){if(name==='text')as2ObjectHost?.textChanged(proxy);return true;}
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
          } else if (ownsTimelineMetadata(name)||['currentFrame', 'totalFrames', '_currentframe', '_totalframes', '_root', '_parent', 'root', 'parent'].includes(name)) {
            throw new Error(`${name} is read-only.`);
          } else return dynamicSet(name,value);
          return true;
        },
      });
      references.set(proxy,{path:path?[...path]:null,objectRef,key,lifetime,nativeHas,as2TimelineRef,nativeKeys:()=>[...Object.keys(methods),...Object.keys(propertyMap),'onEnterFrame','onPress','onRelease','onReleaseOutside','onRollOver','onRollOut','onDragOver','onDragOut','_root','_parent','_visible','_currentframe','_totalframes'],customKeys:()=>Object.keys(custom)});
      if(display)Object.assign(methods,display.methods(proxy));
      if(eventHub)Object.assign(methods,eventHub.methods(proxy));
      // AS2 gotoAndPlay("Scene 2", frame) names the scene first.
      const as2Goto = command => (...args) => args.length >= 2 ? goto(command, args[1], true, args[0]) : goto(command, args[0], true);
      as2.register(methods.gotoAndPlay, proxy, 'gotoAndPlay', as2Goto('goto_and_play'));
      as2.register(methods.gotoAndStop, proxy, 'gotoAndStop', as2Goto('goto_and_stop'));
      // AS2 nextScene/prevScene compile to a frame jump that stops the playhead.
      as2.register(methods.nextScene, proxy, 'nextScene', () => sceneStep(1, 'goto_and_stop'));
      as2.register(methods.prevScene, proxy, 'prevScene', () => sceneStep(-1, 'goto_and_stop'));
      proxies.set(key, { lifetime, proxy });
      return proxy;
    }
    function trace(...values) {
      console.log('[NeoFlash]', ...values);
      lastTrace = values.map(value => typeof value === 'string' ? value : String(value)).join(' ').slice(0, 160);
    }
    function dispatchScripts(clickTarget = null) {
      const wasInDispatch = inDispatch;
      inDispatch = true;
      try {
      program?.runInitClips?.(proxyFor([]));
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
          const scope = item.target_ref?proxyForRef(item.target_ref):proxyFor(item.target_path);
          const event = Object.freeze({
            type: typeof item.event === 'string' ? item.event : 'frame',
            target: item.event === 'click' && clickTargetScope ? clickTargetScope : scope,
            currentTarget: scope,
          });
          try {
            const scriptRoot = item.script.language === 'ActionScript2' ? as2RootFor(scope) : proxyFor([]);
            compiled.fn.call(scope, event, scriptRoot, scope._parent, trace,
              scope.play, scope.stop, scope.gotoAndPlay, scope.gotoAndStop, scope.nextFrame, scope.prevFrame, as2.method, as2.value, program, display,
              legacyGlobals,xmlHost);
          } catch (error) {
            const cause = unwrapScriptThrow(error);
            throw new Error(`${compiled.location}: ${describeThrown(cause)}`, { cause });
          }
        }
        attachNewInstances();
      }
      if (player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive timeline controls.');
      as2ObjectHost?.syncTextVariables();
      drainSounds();
      } finally {
        inDispatch = wasInDispatch;
      }
    }
    // Timeline sounds: the Rust Player queues SoundCommands as frames are entered; the audio
    // service schedules them. Resources are decoded once per session before their keyframe.
    let soundsPreloaded=false;
    function drainSounds(){
      if(typeof player.sound_commands_json!=='function')return;
      const commands=JSON.parse(player.sound_commands_json());
      if(!audioHost)return;
      if(!soundsPreloaded){soundsPreloaded=true;const ids=JSON.parse(player.timeline_sound_resources_json());if(ids.length)audioHost.preloadTimeline(ids);}
      if(commands.length)audioHost.timeline(commands);
    }
    // AS2 stopAllSounds() and AS3 SoundMixer.stopAll() stop scripted and timeline sounds.
    function stopTimelineSounds(){if(typeof player.stop_all_sounds==='function'){player.stop_all_sounds();drainSounds();}}
    function stopAllSounds(){audioHost?.stopAllLegacySounds();stopTimelineSounds();}
    const SoundMixer=Object.freeze({stopAll(){audioHost?.stopAllChannels();stopTimelineSounds();}});
    function resize() { preview.resize(); }
    function font(kind) { return `${kind.font_size}px ${JSON.stringify(kind.font_family)}, sans-serif`; }
    function bitmapMatrix(item){
      if(item.kind.type!=='bitmap_pixels')return item.transform;
      const [width,height]=logicalSize(),sx=canvas.width/width,sy=canvas.height/height,[a,b,c,d,e,f]=item.transform;
      const m=bitmapHost.cache.transform([a*sx,b*sy,c*sx,d*sy,e*sx,f*sy],item.kind.pixel_snapping);
      return [m[0]/sx,m[1]/sy,m[2]/sx,m[3]/sy,m[4]/sx,m[5]/sy];
    }
    function render() {
      let snapshot=graphMode?JSON.parse(player.runtime_snapshot_json()):null;
      if(snapshot&&typeof videoHost!=='undefined'&&videoHost)snapshot=videoHost.prepareSnapshot(snapshot);
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      assertUnfilteredEditing(stageHost?.stage.focus);
      if(snapshot)textInputHost?.update(snapshot);
      const [width,height]=logicalSize();
      if(typeof player.as2_settings_command==='function'&&display){
        const quality=display.as2GlobalPropRef('_quality').get();
        context.imageSmoothingEnabled=quality!=='LOW';
        if('imageSmoothingQuality' in context)context.imageSmoothingQuality=quality==='BEST'?'high':quality==='MEDIUM'?'low':'medium';
      }
      context.setTransform(canvas.width / width, 0, 0, canvas.height / height, 0, 0);
      context.clearRect(0, 0, width, height);
      context.fillStyle = color(project.background);
      context.fillRect(0, 0, width, height);
      bitmapHost?.cache.beginFrame();
      const painter=createScenePainter({global,paths,gradients,clippings,bitmapHost,textHost,filterHost,videoHost,context,matrix:bitmapMatrix,hidden:ref=>Boolean(textInputHost?.hidden(ref))});
      const {clippedBounds,objects}=painter.draw(context,snapshot||{draws:scene,objects:[],semantics:[],clips:[],filter_groups:[]},{width,height});
      const focusBounds=display?.as2FocusBounds?.();if(focusBounds){context.save();context.strokeStyle='#ffcc00';context.lineWidth=2;context.strokeRect(focusBounds.x,focusBounds.y,focusBounds.width,focusBounds.height);context.restore();}
      const semanticScene=snapshot?scene.map(item=>{const ancestry_refs=[];for(let node=objects.get(JSON.stringify(item.owner_ref));node;node=objects.get(JSON.stringify(node.parent_ref)))ancestry_refs.push(node.object_ref);return {...item,...(clippedBounds.has(item)?{clipped_bounds:clippedBounds.get(item)}:{}),transform:bitmapMatrix(item),ancestry_refs,kind:item.kind.type==='bitmap_pixels'?{...item.kind,alpha_bounds:bitmapHost.cache.bounds(item.kind)}:item.kind.type==='text_field'?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind};}):scene;
      const targetCount = semanticLayer.update(snapshot?snapshot.semantics.filter(node=>Boolean(semanticRoute(node,snapshot.objects))):JSON.parse(player.accessibility_json()), semanticScene, {...project,width,height}, {x:preview.view.scale,y:preview.view.scaleY||preview.view.scale}, (line, kind) => { context.font = font(kind); return context.measureText(line).width; });
      bitmapHost?.cache.endFrame();
      if (targetCount) canvas.setAttribute('aria-hidden', 'true'); else canvas.removeAttribute('aria-hidden');
      status.textContent = `${player.current_frame() + 1} / ${player.total_frames()} · ${runtimeFrameRate} fps`;
      if (traceElement.textContent !== lastTrace) traceElement.textContent = lastTrace;
      toggle.textContent = player.is_playing() ? 'Pause' : 'Play';
    }
    function hitAt(event,details=false) {
      let snapshot=graphMode?JSON.parse(player.runtime_snapshot_json()):null;
      if(snapshot&&typeof videoHost!=='undefined'&&videoHost)snapshot=videoHost.prepareSnapshot(snapshot);
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      const clips=snapshot?.clips?.length?global.NeoFlashPaths.clipTable(snapshot.clips):new Map();
      const rect = canvas.getBoundingClientRect();
      const [width,height]=logicalSize();
      const x = (event.clientX - rect.left) * width / rect.width;
      const y = (event.clientY - rect.top) * height / rect.height;
      if(typeof display!=='undefined'&&display?.pickDetails){const result=x>=0&&y>=0&&x<=width&&y<=height?display.pickDetails(x,y):null;return details?result:result?references.get(result.target).objectRef:null;}
      for (let index = scene.length - 1; index >= 0; index--) {
        const item = scene[index];
        if (item.alpha <= 0) continue;
        if(item.clip_ids?.length&&!global.NeoFlashPaths.clipsContain(context,item.clip_ids,clips,x,y,paths))continue;
        const local = localPoint(bitmapMatrix(item), x, y);
        if (!local) continue;
        if (hitShape(item.kind, ...local, (line, kind) => {
          context.font = font(kind);
          return context.measureText(line).width;
        }, (kind, x, y) => global.NeoFlashPaths.contains(context, kind, x, y, paths.get(kind.commands)), (kind,x,y)=>bitmapHost?.cache.hit(kind,x,y)||false,(field,x,y)=>textHost?.hit(field,x,y)||false)) return graphMode?item.owner_ref:item.instance_path;
      }
      return null;
    }
    canvas.addEventListener('click', event => {
      if (failed || preview.panMode || preview.suppressClick) return;
      try {
        const hit=hitAt(event,true),target=hit?.target||(hit?(graphMode?proxyForRef(hit):proxyFor(hit)):null),authored=hit?.owner||target;
        if(stageHost?.handlePointer('click',event,target||stageHost.stage,true)===false)return;
        if(target&&display.alive(target)&&display.alive(authored)){
          if(graphMode&&typeof player.object_click==='function')player.object_click(references.get(authored).key);
          else {
            let owner=authored,node=display.descriptor(owner);
            while(graphMode&&node.authored_path===null){owner=display.parentSpace(owner);if(!owner||owner===stageHost?.stage)break;node=display.descriptor(owner);}
            const path=graphMode?node.authored_path:node.path;
            if(path)player.click(JSON.stringify(path));
          }
          dispatchScripts(authored);
          render();
        }

      } catch (error) { fail(error); }
    });
    canvas.addEventListener('pointermove',event=>{const hit=hitAt(event);pointerOverContent=Boolean(hit&&display.handCursor(graphMode?proxyForRef(hit):proxyFor(hit)));updateCursor();});
    canvas.addEventListener('pointerleave', () => { pointerOverContent=false;updateCursor(); });
    toggle.addEventListener('click', () => {
      player.set_playing(!player.is_playing());
      try { drainSounds(); } catch (error) { fail(error); }
      lastTime = null;
      accumulated = 0;
      render();
    });
    document.getElementById('restart').addEventListener('click', () => {
      failed = false;
      errorElement.style.display = 'none';
      accumulated = 0; lastTime = null;
      try { gradients.clear();clippings.clear();resetProject(); render(); } catch (error) { fail(error); }
    });
    document.getElementById('audio-toggle')?.addEventListener('click',()=>{
      if(failed)return;
      const services=[audioHost,videoHost].filter(host=>host&&host.status().state!=='idle'),mute=services.length&&services.every(host=>{const state=host.status();return state.unlocked&&!state.muted&&state.state!=='awaiting_gesture';});
      Promise.all(services.map(host=>mute?host.setMuted(true):host.resumeFromGesture())).catch(audioFailure);
    });
    global.addEventListener('resize', () => { resize(); render(); });
    document.addEventListener('visibilitychange', () => { lastTime = null; accumulated = 0; });
    document.getElementById('zoom').addEventListener('change', event => preview.setMode(event.target.value));
    const panButton = document.getElementById('pan');
    panButton.addEventListener('click', () => { preview.setPan(!preview.panMode); panButton.setAttribute('aria-pressed', String(preview.panMode)); updateCursor(); });
    global.addEventListener('pagehide', event => { storageHost?.flushAll();if(!event.persisted){gradients.clear();clippings.clear();disposeFamily();} });
    new ResizeObserver(() => { resize(); render(); }).observe(viewport);
    stopAsyncOnFailure=()=>{as2PrintHost?.reset();as2BoxingHost?.reset();as2StageHost?.reset();as2ConnectionsHost?.reset();as2DepthManagerHost?.reset();as2TransitionsHost?.reset();as2MediaHost?.reset();as2ServicesHost?.reset();as2FilesHost?.reset();as2NetworkHost?.reset();as2LocaleHost?.reset();as2TextHost?.reset();as2BindingHost?.reset();as2DataHost?.reset();as2ObjectHost?.reset();gradients.clear();clippings.clear();filterHost?.renderer.clear();externalHost?.reset();textInputHost?.reset();storageHost?.reset();loaderHost?.reset();xmlHost?.reset();videoHost?.reset();as2CaptureHost?.reset();audioHost?.reset();netHost?.reset();stopTimers();binaryHost?.reset();};
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
    global.NeoFlashPlayer = Object.freeze({ get root() { return proxyFor([]); }, pause: () => { player.set_playing(false); drainSounds(); }, resume: () => player.set_playing(true), restart: () => document.getElementById('restart').click() });
  }
  boot().catch(fail);
})(globalThis);
