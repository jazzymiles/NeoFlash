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
  // The Stage and BitmapData.draw share this exact painter. Rust supplies
  // the authoritative snapshot; this adapter never reconstructs a scene graph.
  function createScenePainter({global,paths,gradients,clippings,bitmapHost,textHost,filterHost,context,matrix=item=>item.transform,hidden=()=>false,snapBitmaps=true}) {
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
          gradients.draw(context,kind,paths.get(kind.commands));
        } else if (kind.type === 'bitmap_pixels') {
          if(!bitmapHost)throw new Error('Bitmap runtime is missing. Re-export this project.');
          bitmapHost.cache.draw(context,snapBitmaps?kind:{...kind,pixel_snapping:'never'});
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
        else if(kind.type==='bitmap_pixels')kind={...kind,alpha_bounds:{left:0,top:0,right:kind.width,bottom:kind.height}};
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
      const composite=(target,surface,bounds,clip_ids)=>{
        const draw=canvas=>canvas.drawImage(surface,bounds.left,bounds.top);
        if(clip_ids.length)clippings.draw(target,{clip_ids},clips,paths,draw,{bounds});else draw(target);
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
    const player = new NeoFlashWasm.WebPlayer(JSON.stringify(project));
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
    let display = null, eventHub = null, stageHost = null, legacyHost = null, legacyGlobals = null, timerHost = null, binaryHost = null, netHost = null, bitmapHost = null, textHost = null, filterHost = null, audioHost = null, xmlHost = null, loaderHost = null, amfHost = null, storageHost = null, textInputHost = null, program = null;
    let timerRenderGeneration=0, capturePainter=null;
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
    function netFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Resource callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {textInputHost?.reset();storageHost?.reset();loaderHost?.reset();xmlHost?.reset();audioHost?.reset();netHost?.reset();stopTimers();throw failure;}
    }
    function audioFailure(error){
      const cause=unwrapScriptThrow(error),failure=new Error(`Audio callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {audioHost?.reset();throw failure;}
    }
    function updateAudioStatus(state){
      if(typeof document==='undefined')return;
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
      for(let current=node;current;current=byRef.get(JSON.stringify(current.parent_ref))){
        if(current.filters?.length||JSON.stringify(current.object_ref)===addedKey)throw new TypeError('Native TextField editing inside a filtered subtree is not supported. Clear its ancestor filters or move focus before editing.');
      }
    }
    function initializeFamily() {
      if (!global.NeoFlashDisplay || typeof player.display_nodes_json !== 'function') return;
      if(global.NeoFlashBitmap)bitmapHost=global.NeoFlashBitmap.create({player,geometry:global.NeoFlashGeom,as2,
        getBinary:()=>binaryHost,getFilters:()=>filterHost,
        displayRef:source=>{const ref=references.get(source);if(!ref)return null;display.descriptor(source);return ref.objectRef;},
        paintScene:(target,snapshot)=>{
          if(!global.NeoFlashPaths)throw new Error('BitmapData.draw requires the shared path renderer.');
          if(!capturePainter)capturePainter=createScenePainter({global,paths:new global.NeoFlashPaths.PathCache(),
            gradients:new global.NeoFlashPaths.GradientRenderer({createCanvas:()=>bitmapHost.createCanvas(1,1)}),
            clippings:new global.NeoFlashPaths.ClipRenderer({createCanvas:()=>bitmapHost.createCanvas(1,1)}),bitmapHost,textHost,filterHost,context:target,snapBitmaps:false});
          bitmapHost.cache.beginFrame();try{capturePainter.draw(target,snapshot,{semantic:false});}finally{bitmapHost.cache.endFrame();}
        }});
      if(global.NeoFlashFilters&&typeof player.filter_rgba==='function')filterHost=global.NeoFlashFilters.create({player});
      if(global.NeoFlashText&&typeof player.object_text_info==='function')textHost=global.NeoFlashText.create({player,onUpdate:ref=>textInputHost?.sync(ref),measureText:(text,format)=>{if(typeof context==='undefined')throw new Error('Text layout requires the Canvas font measurement service.');context.font=global.NeoFlashText.cssFont(format);return context.measureText(text).width;}});
      display=global.NeoFlashDisplay.create({player,proxyFor,proxyForRef,onMutation:deliverDisplayEvents,project:typeof project==='undefined'?undefined:project,prepareLinked,getBitmap:()=>bitmapHost,getText:()=>textHost,getLoader:()=>loaderHost,getFilters:()=>filterHost,beforeFilters:(target,specs)=>{if(specs.length)assertUnfilteredEditing(stageHost?.stage.focus,target);},referenceOf:target=>references.get(target),getStage:()=>stageHost?.stage||null,
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
          onFocusChanged:(next,previous)=>textInputHost?.focusChanged(next,previous),ownsTextInput:node=>textInputHost?.owns(node),canFocus:value=>{if(textInputHost&&!textInputHost.canFocus(value))return false;assertUnfilteredEditing(value);return true;},
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
      if(global.NeoFlashTextInput&&textHost&&stageHost&&typeof player.object_text_input==='function'){
        textInputHost=global.NeoFlashTextInput.create({player,text:textHost,display,events:global.NeoFlashEvents,hub:eventHub,stage:stageHost,referenceOf:target=>references.get(target),proxyForRef,
          canvas:typeof canvas==='undefined'?null:canvas,surface:typeof document==='undefined'?null:document.getElementById('stage-surface'),document:typeof document==='undefined'?null:document,project:typeof project==='undefined'?null:project,
          onError:inputFailure,afterInput:afterTimerEvent,onClick:(target,event)=>{stageHost.handlePointer('click',event,target);if(display.alive(target)){player.object_click(references.get(target).key);dispatchScripts(target);if(typeof context!=='undefined')render();}}});
        legacyGlobals=Object.freeze({...legacyGlobals,Selection:textInputHost.Selection});
      }
      if(global.NeoFlashTimers&&global.NeoFlashEvents)timerHost=global.NeoFlashTimers.create({events:global.NeoFlashEvents,
        onError:timerFailure,afterEvent:afterTimerEvent,requestRender:requestTimerRender});
      if(global.NeoFlashByteArray)binaryHost=global.NeoFlashByteArray.create();
      if(global.NeoFlashAMF&&binaryHost&&typeof player.amf_encode==='function'){
        amfHost=global.NeoFlashAMF.create({binary:binaryHost,encode:(format,graph)=>player.amf_encode(format,graph),decode:(format,bytes)=>player.amf_decode(format,bytes),getProgram:()=>program,isUnsupported:value=>references.has(value)});
        binaryHost.setObjectCodec(amfHost);
        if(global.NeoFlashStorage&&global.NeoFlashEvents){
          storageHost=global.NeoFlashStorage.create({amf:amfHost,events:global.NeoFlashEvents,onError:error=>console.warn(error)});
          legacyGlobals=Object.freeze({...legacyGlobals,SharedObject:storageHost.LegacySharedObject});
        }
      }
      if(global.NeoFlashNet&&binaryHost&&global.NeoFlashEvents){
        netHost=global.NeoFlashNet.create({events:global.NeoFlashEvents,binary:binaryHost,resources:typeof project==='undefined'?[]:project.resources||[],onError:netFailure,onAfterDispatch:afterTimerEvent});
        legacyGlobals=Object.freeze({...legacyGlobals,LoadVars:netHost.LoadVars});
      }
      if(global.NeoFlashLoader&&netHost&&binaryHost&&global.NeoFlashEvents){
        loaderHost=global.NeoFlashLoader.create({player,events:global.NeoFlashEvents,net:netHost,binary:binaryHost,display,proxyForRef,referenceOf:target=>references.get(target),onMutation:deliverDisplayEvents,onError:netFailure,afterEvent:afterTimerEvent});
        legacyGlobals=Object.freeze({...legacyGlobals,MovieClipLoader:loaderHost.MovieClipLoader});
      }
      if(global.NeoFlashAudio&&netHost&&binaryHost&&global.NeoFlashEvents){
        audioHost=global.NeoFlashAudio.create({events:global.NeoFlashEvents,net:netHost,binary:binaryHost,onError:audioFailure,afterEvent:afterTimerEvent,onState:updateAudioStatus});
        legacyGlobals=Object.freeze({...legacyGlobals,Sound:audioHost.LegacySound});
      }
      if(global.NeoFlashXML){
        xmlHost=global.NeoFlashXML.create({parse:source=>{const wasm=typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm;if(typeof wasm?.parse_xml_json!=='function')throw new Error('XML parser is missing. Re-export this project.');return wasm.parse_xml_json(source);},net:netHost,onError:netFailure});
        legacyGlobals=Object.freeze({...legacyGlobals,XML:xmlHost.LegacyXML,XMLNode:xmlHost.XMLNode});
      }
      if(textHost)legacyGlobals=Object.freeze({...legacyGlobals,TextFormat:textHost.TextFormat});
      initializeProgram();
    }
    function initializeProgram() {
      if(!display)return;
      const builtins={...display.builtins};
      builtins.__nfTrace=trace;
      builtins.__nfAs2Method=as2.method;builtins.__nfAs2Value=as2.value;
      builtins.__nfDisplay=display;builtins.__nfLegacy=legacyGlobals;
      if(xmlHost){builtins.__nfXML=xmlHost;builtins.XML=xmlHost.XML;builtins.XMLList=xmlHost.XMLList;}
      if(global.NeoFlashEvents)for(const name of ['Event','MouseEvent','KeyboardEvent','FocusEvent','TextEvent','EventDispatcher','EventPhase'])builtins['flash.events.'+name]=global.NeoFlashEvents[name];
      if(global.NeoFlashStage)builtins['flash.display.Stage']=global.NeoFlashStage.Stage;
      if(timerHost){builtins['flash.utils.Timer']=timerHost.Timer;builtins['flash.events.TimerEvent']=timerHost.TimerEvent;builtins['flash.utils.getTimer']=timerHost.getTimer;}
      if(bitmapHost){builtins['flash.display.BitmapData']=bitmapHost.BitmapData;builtins['flash.display.PixelSnapping']=bitmapHost.PixelSnapping;builtins['flash.display.PNGEncoderOptions']=bitmapHost.PNGEncoderOptions;}
      if(textHost){builtins['flash.text.TextFormat']=textHost.TextFormat;builtins['flash.text.TextFormatAlign']=textHost.TextFormatAlign;builtins['flash.text.TextFieldType']=textHost.TextFieldType;}
      if(filterHost)for(const name of ['BitmapFilter','BlurFilter','GlowFilter','DropShadowFilter','ColorMatrixFilter','BitmapFilterQuality'])builtins['flash.filters.'+name]=filterHost[name];
      if(storageHost)for(const name of ['SharedObject','SharedObjectFlushStatus','ObjectEncoding'])builtins['flash.net.'+name]=storageHost[name];
      if(loaderHost)for(const name of ['Loader','LoaderInfo'])builtins['flash.display.'+name]=loaderHost[name];
      if(audioHost)for(const name of ['Sound','SoundChannel','SoundTransform'])builtins['flash.media.'+name]=audioHost[name];
      if(binaryHost){builtins['flash.utils.ByteArray']=binaryHost.ByteArray;builtins['flash.utils.Endian']=binaryHost.Endian;}
      if(netHost){
        for(const name of ['URLRequest','URLRequestHeader','URLRequestMethod','URLVariables','URLLoader','URLLoaderDataFormat'])builtins['flash.net.'+name]=netHost[name];
        for(const name of ['ProgressEvent','IOErrorEvent','HTTPStatusEvent'])builtins['flash.events.'+name]=netHost[name];
      }
      program=typeof bundle!=='undefined'&&bundle.program ? new Function('__nfBuiltins',bundle.program.code)(builtins) : {
        type:name=>{if(!builtins[name])throw new TypeError(`Unsupported built-in class ${name}.`);return builtins[name];},
        is:(value,name)=>Boolean(builtins[name]?.__nfIs?.(value)),
      };
    }
    function resetProject() {
      capturePainter?.clear();capturePainter=null;
      filterHost?.renderer.clear();
      textInputHost?.reset();storageHost?.reset();loaderHost?.reset();xmlHost?.reset();audioHost?.reset();netHost?.reset();stopTimers();binaryHost?.reset();bitmapHost?.reset();textHost?.reset();
      proxies.clear(); attachedClasses.clear();display?.clear();eventHub?.clear();stageHost?.reset();legacyHost?.reset();lastTrace = '';
      player.reset();
      initializeProgram();
      dispatchScripts();
    }
    function disposeFamily() {
      capturePainter?.clear();capturePainter=null;
      filterHost?.renderer.clear();
      textInputHost?.dispose();storageHost?.dispose();loaderHost?.dispose();xmlHost?.dispose();audioHost?.dispose();netHost?.dispose();timerRenderGeneration++;timerHost?.dispose();binaryHost?.dispose();bitmapHost?.dispose();textHost?.dispose();
      legacyHost?.dispose();stageHost?.dispose();eventHub?.clear();display?.clear();
    }
    function prepareLinked(receiver,allocatedByProgram) {
      if(!program?.attach&&display.descriptor(receiver).class_name)throw new Error('Linked symbol class construction requires an initialized project program; creation during static field initialization is not supported yet.');
      if(allocatedByProgram){const ref=references.get(receiver);attachedClasses.set(ref.key,ref.lifetime);}
      attachNewInstances(receiver);
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
        if(graphMode&&node.authored_path===null&&!node.class_name)continue;
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
      }
    }
    function drainDisplayEvents() {
      if(graphMode&&typeof player.display_events_json==='function')deliverDisplayEvents(JSON.parse(player.display_events_json()).events);
    }
    function activateSemantic(node) {
      const accepted=node.owner_ref&&typeof player.object_activate_accessible==='function'
        ?player.object_activate_accessible(JSON.stringify(node.owner_ref))
        :player.activate_accessible(JSON.stringify(node.instance_path),node.lifetime_id);
      if(!accepted)return false;
      const target=node.owner_ref?proxyForRef(node.owner_ref):proxyFor(node.instance_path);
      // Native semantic buttons have one click activation and no Canvas pointerdown.
      // Deliver the legacy press action once, alongside the AS3 click route.
      display?.press(target);
      if(stageHost&&display.alive(target)){stageHost.stage.focus=target;stageHost.handlePointer('click',{},target);}
      dispatchScripts(target);if(typeof context!=='undefined')render();return true;
    }
    function interactiveTarget(hit) {
      let target=graphMode?proxyForRef(hit):proxyFor(hit);
      while(target&&target!==stageHost?.stage){const node=display.descriptor(target);if(node.owner!=='shape'&&node.owner!=='graphic'&&node.owner!=='bitmap')return target;target=display.parentSpace(target);}
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
          const scope = item.target_ref?proxyForRef(item.target_ref):proxyFor(item.target_path);
          const event = Object.freeze({
            type: typeof item.event === 'string' ? item.event : 'frame',
            target: item.event === 'click' && clickTargetScope ? clickTargetScope : scope,
            currentTarget: scope,
          });
          try {
            compiled.fn.call(scope, event, proxyFor([]), scope._parent, trace,
              scope.play, scope.stop, scope.gotoAndPlay, scope.gotoAndStop, scope.nextFrame, scope.prevFrame, as2.method, as2.value, program, display,
              item.script.language==='action_script2'?legacyGlobals:undefined,xmlHost);
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
    function bitmapMatrix(item){
      if(item.kind.type!=='bitmap_pixels')return item.transform;
      const sx=canvas.width/project.width,sy=canvas.height/project.height,[a,b,c,d,e,f]=item.transform;
      const m=bitmapHost.cache.transform([a*sx,b*sy,c*sx,d*sy,e*sx,f*sy],item.kind.pixel_snapping);
      return [m[0]/sx,m[1]/sy,m[2]/sx,m[3]/sy,m[4]/sx,m[5]/sy];
    }
    function render() {
      const snapshot=graphMode?JSON.parse(player.runtime_snapshot_json()):null;
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      assertUnfilteredEditing(stageHost?.stage.focus);
      if(snapshot)textInputHost?.update(snapshot);
      context.setTransform(canvas.width / project.width, 0, 0, canvas.height / project.height, 0, 0);
      context.clearRect(0, 0, project.width, project.height);
      context.fillStyle = color(project.background);
      context.fillRect(0, 0, project.width, project.height);
      bitmapHost?.cache.beginFrame();
      const painter=createScenePainter({global,paths,gradients,clippings,bitmapHost,textHost,filterHost,context,matrix:bitmapMatrix,hidden:ref=>Boolean(textInputHost?.hidden(ref))});
      const {clippedBounds,objects}=painter.draw(context,snapshot||{draws:scene,objects:[],semantics:[],clips:[],filter_groups:[]},{width:project.width,height:project.height});
      const semanticScene=snapshot?scene.map(item=>{const ancestry_refs=[];for(let node=objects.get(JSON.stringify(item.owner_ref));node;node=objects.get(JSON.stringify(node.parent_ref)))ancestry_refs.push(node.object_ref);return {...item,...(clippedBounds.has(item)?{clipped_bounds:clippedBounds.get(item)}:{}),transform:bitmapMatrix(item),ancestry_refs,kind:item.kind.type==='bitmap_pixels'?{...item.kind,alpha_bounds:bitmapHost.cache.bounds(item.kind)}:item.kind.type==='text_field'?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind};}):scene;
      const targetCount = semanticLayer.update(snapshot?snapshot.semantics:JSON.parse(player.accessibility_json()), semanticScene, project, preview.view.scale, (line, kind) => { context.font = font(kind); return context.measureText(line).width; });
      bitmapHost?.cache.endFrame();
      if (targetCount) canvas.setAttribute('aria-hidden', 'true'); else canvas.removeAttribute('aria-hidden');
      status.textContent = `${player.current_frame() + 1} / ${player.total_frames()} · ${runtimeFrameRate} fps`;
      if (traceElement.textContent !== lastTrace) traceElement.textContent = lastTrace;
      toggle.textContent = player.is_playing() ? 'Pause' : 'Play';
    }
    function hitAt(event) {
      const snapshot=graphMode?JSON.parse(player.runtime_snapshot_json()):null;
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      const clips=snapshot?.clips?.length?global.NeoFlashPaths.clipTable(snapshot.clips):new Map();
      const rect = canvas.getBoundingClientRect();
      const x = (event.clientX - rect.left) * project.width / rect.width;
      const y = (event.clientY - rect.top) * project.height / rect.height;
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
        const hit=hitAt(event),target=hit?(graphMode?proxyForRef(hit):proxyFor(hit)):null;
        stageHost?.handlePointer('click',event);
        if(target&&display.alive(target)){
          if(graphMode&&typeof player.object_click==='function')player.object_click(references.get(target).key);
          else {
            let owner=target,node=display.descriptor(owner);
            while(graphMode&&node.authored_path===null){owner=display.parentSpace(owner);if(!owner||owner===stageHost?.stage)break;node=display.descriptor(owner);}
            const path=graphMode?node.authored_path:node.path;
            if(path)player.click(JSON.stringify(path));
          }
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
      try { gradients.clear();clippings.clear();resetProject(); render(); } catch (error) { fail(error); }
    });
    document.getElementById('audio-toggle')?.addEventListener('click',()=>{
      if(!audioHost||failed)return;
      const state=audioHost.status();
      const action=state.unlocked&&!state.muted&&state.state!=='awaiting_gesture'?audioHost.setMuted(true):audioHost.resumeFromGesture();
      action.catch(audioFailure);
    });
    global.addEventListener('resize', () => { resize(); render(); });
    document.addEventListener('visibilitychange', () => { lastTime = null; accumulated = 0; });
    document.getElementById('zoom').addEventListener('change', event => preview.setMode(event.target.value));
    const panButton = document.getElementById('pan');
    panButton.addEventListener('click', () => { preview.setPan(!preview.panMode); panButton.setAttribute('aria-pressed', String(preview.panMode)); updateCursor(); });
    global.addEventListener('pagehide', event => { storageHost?.flushAll();if(!event.persisted){gradients.clear();clippings.clear();disposeFamily();} });
    new ResizeObserver(() => { resize(); render(); }).observe(viewport);
    stopAsyncOnFailure=()=>{gradients.clear();clippings.clear();filterHost?.renderer.clear();textInputHost?.reset();storageHost?.reset();loaderHost?.reset();xmlHost?.reset();audioHost?.reset();netHost?.reset();stopTimers();binaryHost?.reset();};
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
