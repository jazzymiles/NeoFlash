/* Image Loader family, original implementation.
 * Primary contracts: https://airsdk.dev/reference/actionscript/3.0/flash/display/Loader.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/display/LoaderInfo.html
 * https://open-flash.github.io/mirrors/as2-language-reference/MovieClipLoader.html
 * Complete PNG/JPEG files decode in Rust; shared net transport owns resource/
 * HTTP limits. No SWF/GIF, LoaderContext, security-domain or streaming emulation.
 * A pending load leaves committed content visible until successful replacement.
 * AS3 unload detaches content: retained Bitmap/BitmapData remain alive. Pixel
 * arenas are bounded rather than garbage-collected; dispose pixels or Restart.
 * AS2 replaces a MovieClip's contents and unload leaves it empty. Target paths/
 * levels and native subclassing are outside this profile. Generation checks
 * surround every user callback, including nested load/close/unload/Restart.
 */
(function(global){
  'use strict';
  const unsupported=name=>{throw new TypeError(`${name} is not supported in the PNG/JPEG Loader profile.`);};
  const key=ref=>JSON.stringify(ref);
  function create(options={}){
    const {player,events,net,binary,display,proxyForRef,referenceOf}=options;
    if(!player?.pixel_decode||!net?.loadBytes||!binary?.toBytes||!events?.create||!display)throw new TypeError('Loader requires the shared Rust image arena, display, transport and ByteArray services.');
    const schedule=options.schedule||(fn=>global.queueMicrotask(fn));
    const loaders=new WeakMap(),infos=new WeakMap(),legacy=new WeakMap(),records=new Set(),targets=new Map(),contentInfos=new WeakMap();
    let generation=1,disposed=false,queuedBytes=0,rootRecord=null,fallbackDomain=null;
    const byteLoads=new Set();
    const token=Symbol('LoaderInfo');
    const hub=events.create({weakTargets:true,alive:target=>{const record=infos.get(target);return valid(record);}});
    class CallbackFailure {constructor(cause){this.cause=cause;}}
    function open(){if(disposed)throw new Error('The image loading service is disposed.');}
    function valid(record){return Boolean(record&&!disposed&&record.generation===generation&&(record.isRoot||display.alive(record.target)));}
    function state(target){open();const record=loaders.get(target);if(!valid(record))throw new Error('This Loader belongs to a previous session or a removed display object.');return record;}
    function infoState(info){open();const record=infos.get(info);if(!valid(record))throw new Error('This LoaderInfo belongs to a previous session or a removed display object.');return record;}
    function legacyState(receiver){open();const record=legacy.get(receiver);if(!record||record.generation!==generation)throw new Error('This MovieClipLoader belongs to a previous publication session.');return record;}
    function report(error){if(options.onError)options.onError(error);else throw error;}
    function defaultApplicationDomain(){
      const AppDomain=options.getBuiltin?.('flash.system.ApplicationDomain');
      if(AppDomain?.currentDomain)return AppDomain.currentDomain;
      if(AppDomain){try{return new AppDomain();}catch(_){/* fallback below */}}
      if(!fallbackDomain)fallbackDomain=Object.freeze({parentDomain:null,getDefinition:name=>options.getBuiltin?.(String(name))||null,hasDefinition:name=>Boolean(options.getBuiltin?.(String(name)))});
      return fallbackDomain;
    }
    function createUncaughtErrorEvents(){
      const Uncaught=options.getBuiltin?.('flash.events.UncaughtErrorEvents');
      if(Uncaught){try{return new Uncaught();}catch(_){/* fallback below */}}
      return new events.EventDispatcher();
    }
    function current(op){return valid(op.record)&&op.generation===generation&&op.record.operation===op;}
    function releaseBytes(op){if(op.bytes){queuedBytes-=op.bytes.length;op.bytes=null;byteLoads.delete(op);}}
    function cancel(record){const op=record.operation;if(!op)return;record.operation=null;op.transport?.close();releaseBytes(op);}
    function guarded(fn){try{return fn();}catch(error){throw new CallbackFailure(error);}}
    function notify(op,event){
      if(!current(op))return false;
      guarded(()=>hub.dispatch(op.record.info,event));
      if(current(op))guarded(()=>options.afterEvent?.());
      return current(op);
    }
    function legacyNotify(op,name,args){
      if(!current(op))return false;
      const owner=op.record.legacy;
      // A snapshot determines the current broadcast. Removed listeners do not
      // change this delivery; session/operation changes do cancel it.
      for(const listener of [...owner.listeners]){
        if(!current(op))return false;
        if(referenceOf(listener)&&!display.alive(listener)){owner.listeners.delete(listener);continue;}
        const fn=guarded(()=>listener[name]);if(!current(op))return false;
        if(typeof fn==='function')guarded(()=>Reflect.apply(fn,listener,args));
      }
      if(current(op))guarded(()=>options.afterEvent?.());return current(op);
    }
    function newRecord(target,as2=false,owner=null,isRoot=false){
      const defaultBytes=new Uint8Array([0x46,0x57,0x53,12,8,0,0,0]);
      const record={target,ref:target?referenceOf(target)?.objectRef:null,generation,as2,isRoot,legacy:owner,operation:null,info:null,content:null,pixel:null,
        url:isRoot?(typeof global.location?.href==='string'?global.location.href:'app:///publication.swf'):'',
        bytesLoaded:isRoot?defaultBytes.length:0,bytesTotal:isRoot?defaultBytes.length:0,
        width:isRoot?550:0,height:isRoot?400:0,contentType:isRoot?'application/x-shockwave-flash':null,
        swfVersion:12,actionScriptVersion:3,frameRate:Number(options.getFrameRate?.())||24,
        rawBytes:isRoot?defaultBytes:new Uint8Array(0),
        parameters:{},sharedEvents:!as2?new events.EventDispatcher():null,
        uncaughtErrorEvents:null,
        applicationDomain:null,childSandboxBridge:null,parentSandboxBridge:null};
      if(!as2)record.info=new LoaderInfo(token,record);
      records.add(record);if(record.ref)targets.set(key(record.ref),record);return record;
    }
    function ensureRootInfo(){
      open();
      if(!rootRecord||!valid(rootRecord))rootRecord=newRecord(null,false,null,true);
      return rootRecord.info;
    }
    function Loader(...args){
      open();if(args.length)throw new TypeError('Loader construction takes no arguments.');
      if(new.target&&new.target!==Loader)unsupported('Loader native subclassing');
      const target=proxyForRef(JSON.parse(player.create_display('loader'))),record=newRecord(target);loaders.set(target,record);return target;
    }
    Object.defineProperties(Loader,{__nfIs:{value:value=>loaders.has(value)},__nfInitialize:{value:()=>unsupported('Loader native subclassing')}});
    class LoaderInfo extends events.EventDispatcher {
      constructor(secret,record){super();if(secret!==token)unsupported('LoaderInfo construction; use contentLoaderInfo');infos.set(this,record);Object.assign(this,hub.methods(this));Object.defineProperty(this,'dispatchEvent',{writable:false,configurable:false,value:()=>{throw new TypeError('LoaderInfo.dispatchEvent is forbidden; its events are dispatched by Loader.');}});Object.seal(this);}
      static getLoaderInfoByDefinition(object){
        open();if(object===undefined||object===null)throw new TypeError('LoaderInfo.getLoaderInfoByDefinition requires a non-null object.');
        return ensureRootInfo();
      }
      get loader(){return infoState(this).target;}
      get content(){const record=infoState(this);return record.isRoot?(options.getRootDisplay?options.getRootDisplay():record.content):record.content;}
      get url(){return infoState(this).url;}
      get loaderURL(){infoState(this);return typeof global.location?.href==='string'?global.location.href:'app:///publication.swf';}
      get bytesLoaded(){return infoState(this).bytesLoaded;}
      get bytesTotal(){return infoState(this).bytesTotal;}
      get width(){const record=initialized(this);return record.isRoot&&options.getStageSize?Number(options.getStageSize().width)||record.width:record.width;}
      get height(){const record=initialized(this);return record.isRoot&&options.getStageSize?Number(options.getStageSize().height)||record.height:record.height;}
      get contentType(){return initialized(this).contentType;}
      get actionScriptVersion(){const record=infoState(this);return record.actionScriptVersion||3;}
      get swfVersion(){const record=infoState(this);return record.swfVersion||12;}
      get frameRate(){const record=infoState(this);return record.isRoot?(Number(options.getFrameRate?.())||24):(record.frameRate||Number(options.getFrameRate?.())||24);}
      get sameDomain(){infoState(this);return true;}
      get childAllowsParent(){infoState(this);return true;}
      get parentAllowsChild(){infoState(this);return true;}
      get isURLInaccessible(){infoState(this);return false;}
      get parameters(){return infoState(this).parameters;}
      get sharedEvents(){return infoState(this).sharedEvents;}
      get uncaughtErrorEvents(){const record=infoState(this);if(!record.uncaughtErrorEvents)record.uncaughtErrorEvents=createUncaughtErrorEvents();return record.uncaughtErrorEvents;}
      get applicationDomain(){const record=infoState(this);if(!record.applicationDomain)record.applicationDomain=defaultApplicationDomain();return record.applicationDomain;}
      get bytes(){const record=infoState(this);return binary.fromBytes(record.rawBytes||new Uint8Array(0));}
      get childSandboxBridge(){return infoState(this).childSandboxBridge;}
      set childSandboxBridge(value){infoState(this).childSandboxBridge=value;}
      get parentSandboxBridge(){return infoState(this).parentSandboxBridge;}
      set parentSandboxBridge(value){infoState(this).parentSandboxBridge=value;}
    }
    function initialized(info){const record=infoState(info);if(!record.isRoot&&!record.content)throw new Error('LoaderInfo dimensions and contentType are available after INIT.');return record;}
    Object.defineProperties(LoaderInfo,{__nfIs:{value:value=>infos.has(value)},__nfInitialize:{value:()=>unsupported('LoaderInfo native subclassing')}});
    function begin(record,url){
      cancel(record);record.url=url;record.bytesLoaded=record.bytesTotal=0;
      const op={record,generation,transport:null,status:0};record.operation=op;return op;
    }
    function transportEvent(op,event){
      if(!current(op))return;
      const record=op.record;
      if(event.type==='httpStatus')op.status=event.status;
      if(event.type==='progress'){record.bytesLoaded=event.bytesLoaded;record.bytesTotal=event.bytesTotal;}
      if(record.as2){
        if(event.type==='open')legacyNotify(op,'onLoadStart',[record.target]);
        else if(event.type==='progress')legacyNotify(op,'onLoadProgress',[record.target,record.bytesLoaded,record.bytesTotal]);
        else if(event.type==='ioError')failLoad(op,event.text);
      }else if(event.type==='ioError')failLoad(op,event.text);
      else notify(op,event);
    }
    function failLoad(op,message){
      if(!current(op))return;
      if(op.record.as2)legacyNotify(op,'onLoadError',[op.record.target,String(message),op.status]);
      else notify(op,new net.IOErrorEvent('ioError',false,false,String(message)));
      if(current(op))op.record.operation=null;
    }
    const utf8Decoder=new TextDecoder('utf-8',{fatal:false});
    function parseNeoFlashBundleText(text){
      const trimmed=text.trim();
      if(!trimmed)return null;
      let jsonSource=null;
      const prefixMatch=/^(?:\/\*[\s\S]*?\*\/\s*)*(?:globalThis|window|self)\.NeoFlashBundle\s*=\s*/.exec(trimmed);
      if(prefixMatch){
        jsonSource=trimmed.slice(prefixMatch[0].length).replace(/(?:;\s*)?(?:\/\/#[^\r\n]*\s*)*$/,'').replace(/;\s*$/,'');
      }else if(trimmed.startsWith('{')&&trimmed.includes('"project"')&&trimmed.includes('"symbols"')){
        jsonSource=trimmed;
      }
      if(!jsonSource)return null;
      const parsed=JSON.parse(jsonSource);
      if(parsed&&typeof parsed==='object'&&parsed.project&&typeof parsed.project==='object'&&Array.isArray(parsed.project.symbols)){
        return parsed;
      }
      return null;
    }
    function detectBundleRedirectPath(text,currentUrl){
      const trimmed=text.trim();
      if(!trimmed||!currentUrl)return null;
      let bundleRel=null;
      if(/^<!doctype\s+html|^<html/i.test(trimmed)&&(/neoflash/i.test(trimmed)||/<script\b[^>]*\bsrc=["'][^"']*bundle\.js/i.test(trimmed))){
        const scriptMatch=/<script\b[^>]*\bsrc=["']([^"']*bundle\.js(?:\?[^"']*)?)["']/i.exec(trimmed);
        if(scriptMatch)bundleRel=scriptMatch[1];
      }else if(trimmed.startsWith('{')&&trimmed.includes('"bundle.js"')){
        try{
          const manifest=JSON.parse(trimmed);
          if(Array.isArray(manifest?.files)){
            const entry=manifest.files.find(f=>(typeof f==='string'?f:f?.path||'').endsWith('bundle.js'));
            bundleRel=typeof entry==='string'?entry:entry?.path||null;
          }else if(manifest?.format==='html5'){
            bundleRel='bundle.js';
          }
        }catch(_){}
      }
      if(!bundleRel)return null;
      const clean=String(currentUrl).split('#')[0];
      const qIdx=clean.indexOf('?'),pathPart=qIdx>=0?clean.slice(0,qIdx):clean;
      const slash=pathPart.lastIndexOf('/');
      const dir=slash>=0?pathPart.slice(0,slash+1):'';
      return dir+bundleRel.replace(/^\.\//,'');
    }
    function parseSvgDimensions(svgText){
      const tagMatch=/<svg\b[^>]*>/i.exec(svgText);
      const tag=tagMatch?tagMatch[0]:'';
      const attrNum=name=>{
        const m=new RegExp(`\\b${name}\\s*=\\s*["']\\s*([0-9]+(?:\\.[0-9]+)?)`,'i').exec(tag);
        return m?Number(m[1]):0;
      };
      let w=attrNum('width'),h=attrNum('height');
      if(!(w>0&&h>0)){
        const vb=/\bviewBox\s*=\s*["']\s*[-+0-9.]+\s+[-+0-9.]+\s+([0-9.]+)\s+([0-9.]+)/i.exec(tag);
        if(vb){if(!(w>0))w=Number(vb[1]);if(!(h>0))h=Number(vb[2]);}
      }
      w=Math.max(1,Math.min(4096,Math.round(w||256)));
      h=Math.max(1,Math.min(4096,Math.round(h||256)));
      return [w,h];
    }
    function parseSvgColorHex(svgText){
      const m=/\bfill\s*=\s*["']#([0-9a-fA-F]{6}|[0-9a-fA-F]{3})["']/i.exec(svgText);
      if(!m)return 0xff3b82f6;
      const hex=m[1].length===3?m[1].split('').map(c=>c+c).join(''):m[1];
      return (0xff000000|(parseInt(hex,16)&0xffffff))>>>0;
    }
    function contentType(bytes,as2=false){
      if(bytes.length>=8&&[137,80,78,71,13,10,26,10].every((v,i)=>bytes[i]===v))return 'image/png';
      if(bytes.length>=3&&bytes[0]===255&&bytes[1]===216&&bytes[2]===255)return 'image/jpeg';
      if(bytes.length>=6&&bytes[0]===0x47&&bytes[1]===0x49&&bytes[2]===0x46&&bytes[3]===0x38&&(bytes[4]===0x37||bytes[4]===0x39)&&bytes[5]===0x61)return 'image/gif';
      if(bytes.length>=12&&bytes[0]===0x52&&bytes[1]===0x49&&bytes[2]===0x46&&bytes[3]===0x46&&bytes[8]===0x57&&bytes[9]===0x45&&bytes[10]===0x42&&bytes[11]===0x50)return 'image/webp';
      if(bytes.length>=14&&bytes[0]===0x42&&bytes[1]===0x4d)return 'image/bmp';
      if(bytes.length>=8&&((bytes[0]===0x49&&bytes[1]===0x49&&bytes[2]===0x2a&&bytes[3]===0x00)||(bytes[0]===0x4d&&bytes[1]===0x4d&&bytes[2]===0x00&&bytes[3]===0x2a)))return 'image/tiff';
      if(bytes.length>=8&&(bytes[0]===0x46||bytes[0]===0x43||(as2&&bytes[0]===0x5a))&&bytes[1]===0x57&&bytes[2]===0x53)return 'application/x-shockwave-flash';
      if(bytes.length>=5&&bytes.length<=32*1024*1024){
        const head=utf8Decoder.decode(bytes.subarray(0,Math.min(bytes.length,65536))).replace(/^\uFEFF/,'').trimStart();
        if(/^(?:<\?xml\b[\s\S]*?\?>\s*)?(?:<!DOCTYPE\b[^>]*>\s*)?<svg\b/i.test(head))return 'image/svg+xml';
        if(/^(?:\/\*[\s\S]*?\*\/\s*)*(?:globalThis|window|self)\.NeoFlashBundle\s*=/.test(head)||(head.startsWith('{')&&head.includes('"project"')))return 'application/vnd.neoflash.bundle';
        if((/^<!doctype\s+html|^<html/i.test(head)&&(/neoflash/i.test(head)||/<script\b[^>]*\bsrc=["'][^"']*bundle\.js/i.test(head)))||(head.startsWith('{')&&head.includes('"bundle.js"')))return 'application/vnd.neoflash.publish-entry';
      }
      throw new TypeError('Loader supports PNG, JPEG, GIF, WebP, BMP, TIFF, SVG, SWF and published NeoFlash bundles. Other file formats are not implemented.');
    }
    function parseSwfHeaderFallback(bytes){
      if(bytes.length<8)throw new TypeError('SWF data is truncated.');
      const sig=String.fromCharCode(bytes[0],bytes[1],bytes[2]);
      if(sig!=='FWS'&&sig!=='CWS'&&sig!=='ZWS')throw new TypeError('Unsupported SWF signature.');
      return {swfVersion:bytes[3]||10,actionScriptVersion:3,width:550,height:400,frameRate:24,frameCount:1,backgroundColor:null};
    }
    function findProjectSymbol(url){
      const raw=String(url||'').split('?')[0].split('#')[0];
      const name=raw.startsWith('symbol:')?raw.slice(7).trim():raw.replace(/\.swf$/i,'').split('/').pop();
      if(!name||!Array.isArray(options.project?.symbols))return null;
      return options.project.symbols.find(sym=>sym.kind==='movie_clip'&&(sym.linkage_id===name||(raw.startsWith('symbol:')&&(sym.id===name||sym.name===name))))||null;
    }
    function applyContext(record,context){
      if(context===undefined||context===null)return;
      if(typeof context!=='object')throw new TypeError('LoaderContext must be an object or null.');
      if(context.applicationDomain)record.applicationDomain=context.applicationDomain;
      if(context.parameters&&typeof context.parameters==='object')record.parameters={...context.parameters};
    }
    function detachExistingPublished(record){
      if(!record.publishedMounted||typeof options.unloadPublishedBundle!=='function')return;
      record.publishedMounted=false;
      const packet=options.unloadPublishedBundle(record,true);
      if(packet?.events?.length)guarded(()=>options.onMutation?.(packet.events));
    }
    function commitPixelBytes(op,encodedBytes,rawBytes,mime){
      if(!current(op))return;let pixel=null,committed=false;
      try{
        const record=op.record;
        pixel=JSON.parse(player.pixel_decode(encodedBytes));
        const dimensions=JSON.parse(player.bitmap_info(key(pixel)));
        record.bytesLoaded=record.bytesTotal=rawBytes.length;record.rawBytes=new Uint8Array(rawBytes);
        if(record.as2&&!legacyNotify(op,'onLoadComplete',[record.target,op.status]))return;
        if(!current(op))return;
        detachExistingPublished(record);if(!current(op))return;
        const packet=JSON.parse(player.loader_replace(key(record.ref),key(pixel),record.as2));committed=true;
        record.pixel=pixel;record.content=packet.content_ref?proxyForRef(packet.content_ref):null;
        record.width=dimensions.width;record.height=dimensions.height;record.contentType=mime;
        if(record.content&&!record.as2)contentInfos.set(record.content,record.info);
        guarded(()=>options.onMutation?.(packet.events));if(!current(op))return;
        if(record.as2)legacyNotify(op,'onLoadInit',[record.target]);
        else if(notify(op,new events.Event('init')))notify(op,new events.Event('complete'));
        if(current(op))record.operation=null;
      }finally{if(pixel&&!committed){try{player.pixel_release(key(pixel));}catch(_){/* Restart already retired the arena. */}}}
    }
    function decodeSvgAndCommit(op,bytes){
      const svgText=utf8Decoder.decode(bytes);
      const [w,h]=parseSvgDimensions(svgText);
      if(typeof global.Image==='function'&&typeof global.document?.createElement==='function'){
        const img=new global.Image();
        img.onload=()=>{
          if(!current(op))return;
          try{
            const surface=global.document.createElement('canvas');surface.width=w;surface.height=h;
            const ctx=surface.getContext('2d');if(!ctx)throw new Error('SVG rasterization requires Canvas 2D.');
            ctx.drawImage(img,0,0,w,h);
            const dataUrl=surface.toDataURL('image/png'),comma=dataUrl.indexOf(',');
            const raw=global.atob(comma>=0?dataUrl.slice(comma+1):'');
            const pngBytes=Uint8Array.from(raw,ch=>ch.charCodeAt(0));
            commitPixelBytes(op,pngBytes,bytes,'image/svg+xml');
          }catch(error){
            if(error instanceof CallbackFailure)report(error.cause);
            else if(current(op))try{failLoad(op,error instanceof Error?error.message:String(error));}catch(cbErr){report(cbErr instanceof CallbackFailure?cbErr.cause:cbErr);}
          }
        };
        img.onerror=()=>{if(current(op)){try{failLoad(op,'Failed to decode SVG graphic.');}catch(cbErr){report(cbErr instanceof CallbackFailure?cbErr.cause:cbErr);}}};
        img.src='data:image/svg+xml;charset=utf-8,'+encodeURIComponent(svgText);
        return;
      }
      if(typeof player.bitmap_create!=='function'||typeof player.bitmap_encode_png!=='function'){
        throw new Error('SVG runtime decoding requires Canvas Image or WASM bitmap encoding.');
      }
      const bmd=JSON.parse(player.bitmap_create(w,h,true,parseSvgColorHex(svgText)));
      let pngBytes;
      try{pngBytes=player.bitmap_encode_png(key(bmd),0,0,w,h,false);}
      finally{try{player.bitmap_dispose?.(key(bmd));}catch(_){}}
      commitPixelBytes(op,pngBytes,bytes,'image/svg+xml');
    }
    function decode(op,bytes){
      if(!current(op))return;
      try{
        const record=op.record,mime=contentType(bytes,record.as2);
        if(mime==='application/vnd.neoflash.publish-entry'){
          const text=utf8Decoder.decode(bytes),redirectUrl=detectBundleRedirectPath(text,record.url);
          if(!redirectUrl)throw new TypeError('Published entry HTML/manifest did not reference a NeoFlash bundle.js.');
          op.transport=net.loadBytes(new net.URLRequest(redirectUrl),{
            onEvent:event=>{try{transportEvent(op,event);}catch(error){throw error instanceof CallbackFailure?error.cause:error;}},
            onComplete:nextBytes=>decode(op,nextBytes)
          });
          return;
        }
        if(mime==='application/vnd.neoflash.bundle'){
          const bundleObj=parseNeoFlashBundleText(utf8Decoder.decode(bytes));
          if(!bundleObj)throw new TypeError('Malformed NeoFlash published bundle.');
          if(typeof options.mountPublishedBundle!=='function')throw new Error('Host runtime does not support dynamic NeoFlash bundle mounting.');
          record.bytesLoaded=record.bytesTotal=bytes.length;record.rawBytes=new Uint8Array(bytes);
          if(record.as2&&!legacyNotify(op,'onLoadComplete',[record.target,op.status]))return;
          if(!current(op))return;
          const mounted=options.mountPublishedBundle(record,bundleObj);
          record.publishedMounted=true;record.pixel=null;
          record.content=mounted.contentRef?proxyForRef(mounted.contentRef):(record.as2?record.target:null);
          record.width=mounted.width;record.height=mounted.height;record.frameRate=mounted.frameRate;
          record.contentType='application/x-shockwave-flash';
          if(record.content&&!record.as2)contentInfos.set(record.content,record.info);
          guarded(()=>options.onMutation?.(mounted.events||[]));if(!current(op))return;
          guarded(()=>options.onAfterMountPublished?.());if(!current(op))return;
          if(record.as2)legacyNotify(op,'onLoadInit',[record.target]);
          else if(notify(op,new events.Event('init')))notify(op,new events.Event('complete'));
          if(current(op))record.operation=null;
          return;
        }
        if(mime==='image/svg+xml'){
          decodeSvgAndCommit(op,bytes);
          return;
        }
        if(mime==='application/x-shockwave-flash'){
          if(record.as2){
            record.bytesLoaded=record.bytesTotal=bytes.length;record.rawBytes=new Uint8Array(bytes);record.contentType=mime;
            if(!legacyNotify(op,'onLoadComplete',[record.target,op.status]))return;
            if(!current(op))return;
            detachExistingPublished(record);if(!current(op))return;
            const packet=JSON.parse(player.loader_replace(key(record.ref),'null',true));
            guarded(()=>options.onMutation?.(packet.events));if(!current(op))return;
            const sym=findProjectSymbol(record.url);
            if(sym?.linkage_id&&display.alive(record.target)){try{record.content=record.target.attachMovie(sym.linkage_id,'__nfLoadedClip',0);}catch(_){record.content=record.target;}}
            else record.content=record.target;
            legacyNotify(op,'onLoadInit',[record.target]);
          }else{
            const swfInfo=options.inspectSwfBytes?options.inspectSwfBytes(bytes):parseSwfHeaderFallback(bytes);
            record.bytesLoaded=record.bytesTotal=bytes.length;record.rawBytes=new Uint8Array(bytes);
            if(!current(op))return;
            detachExistingPublished(record);if(!current(op))return;
            const packet=JSON.parse(player.loader_replace_swf(key(record.ref),swfInfo.actionScriptVersion,swfInfo.width,swfInfo.height,swfInfo.backgroundColor!==null&&swfInfo.backgroundColor!==undefined,swfInfo.backgroundColor||0,false));
            record.pixel=null;record.content=packet.content_ref?proxyForRef(packet.content_ref):null;
            record.width=swfInfo.width;record.height=swfInfo.height;record.swfVersion=swfInfo.swfVersion;record.actionScriptVersion=swfInfo.actionScriptVersion;record.frameRate=swfInfo.frameRate;record.contentType=mime;
            if(record.content)contentInfos.set(record.content,record.info);
            guarded(()=>options.onMutation?.(packet.events));if(!current(op))return;
            if(notify(op,new events.Event('init')))notify(op,new events.Event('complete'));
          }
          if(current(op))record.operation=null;
          return;
        }
        commitPixelBytes(op,bytes,bytes,mime);
      }catch(error){
        if(error instanceof CallbackFailure)throw error.cause;
        if(current(op)){try{failLoad(op,error instanceof Error?error.message:String(error));}catch(callbackError){throw callbackError instanceof CallbackFailure?callbackError.cause:callbackError;}}
      }
    }
    function normalizeRequestUrl(rawUrl){
      if(typeof rawUrl!=='string')return rawUrl;
      const hashIdx=rawUrl.indexOf('#'),noHash=hashIdx>=0?rawUrl.slice(0,hashIdx):rawUrl,hash=hashIdx>=0?rawUrl.slice(hashIdx):'';
      const qIdx=noHash.indexOf('?'),pathPart=qIdx>=0?noHash.slice(0,qIdx):noHash,query=qIdx>=0?noHash.slice(qIdx):'';
      if(pathPart.endsWith('/'))return pathPart+'bundle.js'+query+hash;
      return rawUrl;
    }
    function load(record,request,context=null){
      applyContext(record,context);
      if(!net.URLRequest.__nfIs(request))throw new TypeError('Loader.load requires a URLRequest.');
      if(record.as2&&/^symbol:/i.test(String(request.url||''))){
        const sym=findProjectSymbol(request.url);
        if(!sym?.linkage_id){const op=begin(record,request.url);schedule(()=>{if(current(op))failLoad(op,`Embedded symbol "${request.url}" was not found.`);});return;}
        const swfHeader=new Uint8Array([0x46,0x57,0x53,8,8,0,0,0]);
        const op=begin(record,request.url);op.status=200;
        schedule(()=>{if(!current(op))return;try{transportEvent(op,new events.Event('open'));if(current(op))transportEvent(op,new net.ProgressEvent('progress',false,false,swfHeader.length,swfHeader.length));if(current(op))decode(op,swfHeader);}catch(error){if(op.generation===generation&&!disposed){cancel(record);report(error instanceof CallbackFailure?error.cause:error);}}});
        return;
      }
      // The transport snapshots URLRequest before async work. Its validation is
      // synchronous; use the actual request string for metadata, never fetch it.
      const normalizedUrl=normalizeRequestUrl(request.url);
      if(normalizedUrl!==request.url){
        const nextReq=new net.URLRequest(normalizedUrl);
        nextReq.method=request.method;nextReq.data=request.data;nextReq.contentType=request.contentType;nextReq.requestHeaders=request.requestHeaders.slice();
        request=nextReq;
      }
      const op=begin(record,request.url);
      try{op.transport=net.loadBytes(request,{onEvent:event=>{try{transportEvent(op,event);}catch(error){throw error instanceof CallbackFailure?error.cause:error;}},onComplete:bytes=>decode(op,bytes)});}
      catch(error){if(current(op))cancel(record);throw error;}
    }
    function loadBytes(record,value,context=null){
      applyContext(record,context);
      const bytes=binary.toBytes(value);if(bytes.length>16*1024*1024)throw new RangeError('Loader.loadBytes exceeds the 16 MiB encoded image limit.');
      if(byteLoads.size>=32||queuedBytes+bytes.length>64*1024*1024)throw new RangeError('Pending Loader.loadBytes snapshots exceed 32 loads or 64 MiB.');
      const op=begin(record,'');op.bytes=bytes;queuedBytes+=bytes.length;byteLoads.add(op);
      try{schedule(()=>{if(!current(op)){releaseBytes(op);return;}try{transportEvent(op,new events.Event('open'));if(current(op))transportEvent(op,new net.ProgressEvent('progress',false,false,op.bytes.length,op.bytes.length));if(current(op))decode(op,op.bytes);}catch(error){if(op.generation===generation&&!disposed){cancel(record);report(error instanceof CallbackFailure?error.cause:error);}}finally{releaseBytes(op);}});}
      catch(error){cancel(record);throw error;}
    }
    function unload(record,stopDetached=false){
      cancel(record);const hadContent=record.content!==null;
      let packet;
      if(record.publishedMounted&&typeof options.unloadPublishedBundle==='function'){
        record.publishedMounted=false;
        packet=options.unloadPublishedBundle(record,Boolean(stopDetached));
      }else{
        packet=JSON.parse(player.loader_replace(key(record.ref),'null',record.as2));
      }
      record.content=null;record.pixel=null;record.width=record.height=record.bytesLoaded=record.bytesTotal=0;record.contentType=null;record.rawBytes=new Uint8Array(0);
      const op=begin(record,'');
      try{guarded(()=>options.onMutation?.(packet?.events||[]));if(!record.as2&&hadContent&&current(op))notify(op,new events.Event('unload'));}
      catch(error){throw error instanceof CallbackFailure?error.cause:error;}
      finally{if(current(op))record.operation=null;}
    }
    function methods(target){return {
      load:(request,context=null)=>load(state(target),request,context),loadBytes:(bytes,context=null)=>loadBytes(state(target),bytes,context),
      close:()=>cancel(state(target)),unload:()=>unload(state(target),false),unloadAndStop:(gc=true)=>{if(typeof gc!=='boolean')throw new TypeError('Loader.unloadAndStop gc requires a Boolean.');unload(state(target),true);},
      addChild:()=>unsupported('Loader.addChild: content is managed'),addChildAt:()=>unsupported('Loader.addChildAt: content is managed'),
      removeChild:()=>unsupported('Loader.removeChild: content is managed'),removeChildAt:()=>unsupported('Loader.removeChildAt: content is managed'),
      setChildIndex:()=>unsupported('Loader.setChildIndex: content is managed'),swapChildren:()=>unsupported('Loader.swapChildren: content is managed'),swapChildrenAt:()=>unsupported('Loader.swapChildrenAt: content is managed'),
    };}
    function get(target,name){const record=state(target);if(name==='content')return record.content;if(name==='contentLoaderInfo')return record.info;if(name==='uncaughtErrorEvents')return record.info.uncaughtErrorEvents;return undefined;}
    function targetRecord(receiver,target,createRecord){
      const owner=legacyState(receiver);
      if(target===0&&typeof options.getRoot==='function')target=options.getRoot();
      else if(typeof target==='string')target=/^_level0$/i.test(target)&&typeof options.getRoot==='function'?options.getRoot():display.resolveTarget(target,options.getRoot?.());
      const ref=referenceOf(target);
      if(!ref||!display.alive(target)||display.descriptor(target).owner!=='movie_clip')throw new TypeError('MovieClipLoader requires a live MovieClip target.');
      const node=display.descriptor(target);if(node.authored_path?.length===0)throw new TypeError('MovieClipLoader cannot replace the publication root.');
      let record=targets.get(key(ref.objectRef));
      if(record&&!record.as2)throw new TypeError('MovieClipLoader cannot replace an AS3 Loader.');
      if(!record||!valid(record)){if(!createRecord)return null;record=newRecord(target,true,owner);}
      if(createRecord){cancel(record);record.legacy=owner;}return record;
    }
    class MovieClipLoader {
      constructor(...args){open();if(args.length)throw new TypeError('MovieClipLoader construction takes no arguments.');legacy.set(this,{generation,listeners:new Set([this]),checkPolicyFile:false});}
      addListener(listener){const record=legacyState(this);if(!listener||!['object','function'].includes(typeof listener))return false;if(record.listeners.has(listener))return false;if(record.listeners.size>=1024)throw new RangeError('MovieClipLoader supports at most 1024 listeners.');record.listeners.add(listener);return true;}
      removeListener(listener){return legacyState(this).listeners.delete(listener);}
      loadClip(url,target){if(typeof url!=='string')throw new TypeError('MovieClipLoader.loadClip URL requires a string.');const record=targetRecord(this,target,true);load(record,new net.URLRequest(url));return true;}
      unloadClip(target){const record=targetRecord(this,target,false);if(!record)return false;unload(record);return true;}
      getProgress(target){const record=targetRecord(this,target,false);return {bytesLoaded:record?.bytesLoaded||0,bytesTotal:record?.bytesTotal||0};}
      get checkPolicyFile(){return legacyState(this).checkPolicyFile;}set checkPolicyFile(value){const record=legacyState(this);if(typeof value!=='boolean')throw new TypeError('MovieClipLoader.checkPolicyFile requires a Boolean.');record.checkPolicyFile=value;}
    }
    Object.defineProperties(MovieClipLoader,{__nfIs:{value:value=>legacy.has(value)},__nfInitialize:{value:()=>unsupported('MovieClipLoader native subclassing')}});
    let simpleReceiver=null;
    function simpleLoader(){
      if(!simpleReceiver){
        simpleReceiver=new MovieClipLoader();
        simpleReceiver.onLoadInit=target=>{if(display.alive(target))display.notifyData?.(target);};
        simpleReceiver.onLoadError=(target,message)=>report(new TypeError('MovieClip.loadMovie failed: '+message));
      }
      return simpleReceiver;
    }
    function trackedMovie(target){
      const ref=referenceOf(target);
      if(!ref||!display.alive(target)||display.descriptor(target).owner!=='movie_clip')throw new TypeError('MovieClip byte counts require a live MovieClip target.');
      const node=display.descriptor(target);
      if(node.authored_path?.length===0){ensureRootInfo();return rootRecord;}
      const record=targets.get(key(ref.objectRef));
      if(record&&valid(record))return record;
      return {bytesLoaded:0,bytesTotal:0};
    }
    function urlForDisplay(target){
      const ref=referenceOf(target);
      if(!ref||!display.alive(target))return null;
      const record=targets.get(key(ref.objectRef));
      return record&&valid(record)&&record.url?record.url:null;
    }
    function as2MovieMethods(target){return {
      loadMovie(url,method){
        if(typeof url!=='string')throw new TypeError('MovieClip.loadMovie URL requires String.');
        const request=new net.URLRequest(url);
        if(method!==undefined&&method!==null){
          if(!['GET','POST'].includes(method))throw new TypeError('MovieClip.loadMovie method requires GET or POST.');
          if(typeof options.legacyVariables!=='function')throw new Error('MovieClip form variables require the publication variable adapter.');
          const variables=new net.URLVariables();for(const[name,value]of Object.entries(options.legacyVariables(target)))Object.defineProperty(variables,name,{value,enumerable:true,writable:true,configurable:true});
          request.method=method;request.data=variables;
        }
        load(targetRecord(simpleLoader(),target,true),request);
      },
      loadPublish(url){
        if(typeof url!=='string')throw new TypeError('MovieClip.loadPublish URL requires String.');
        load(targetRecord(simpleLoader(),target,true),new net.URLRequest(normalizeRequestUrl(url)));
      },
      unloadMovie(){unload(targetRecord(simpleLoader(),target,true),true);},
      getBytesLoaded(){return trackedMovie(target).bytesLoaded;},
      getBytesTotal(){return trackedMovie(target).bytesTotal;},
    };}
    function loadPublish(url,target=null){
      open();if(typeof url!=='string'||!url.trim())throw new TypeError('loadPublish requires a non-empty URL string.');
      const normalized=normalizeRequestUrl(url.trim());
      if(target&&loaders.has(target)){
        const rec=state(target);
        return new Promise((resolve,reject)=>{
          const onComplete=()=>{rec.info.removeEventListener('complete',onComplete);rec.info.removeEventListener('ioError',onError);resolve(rec.content);};
          const onError=ev=>{rec.info.removeEventListener('complete',onComplete);rec.info.removeEventListener('ioError',onError);reject(new Error(ev?.text||'loadPublish failed'));};
          rec.info.addEventListener('complete',onComplete);
          rec.info.addEventListener('ioError',onError);
          load(rec,new net.URLRequest(normalized));
        });
      }
      if(target===null||target===undefined){
        const ldr=new Loader();
        const rootObj=options.getRootDisplay?.();
        if(rootObj&&typeof rootObj.addChild==='function'){try{rootObj.addChild(ldr);}catch(_){}}
        return loadPublish(normalized,ldr);
      }
      const mcl=new MovieClipLoader();
      return new Promise((resolve,reject)=>{
        mcl.addListener({
          onLoadInit:clip=>resolve(clip),
          onLoadError:(_clip,msg)=>reject(new Error(msg||'loadPublish failed')),
        });
        mcl.loadClip(normalized,target);
      });
    }
    function reset(){open();for(const record of records){cancel(record);if(record.publishedMounted&&typeof options.unloadPublishedBundle==='function'){try{options.unloadPublishedBundle(record,true);}catch(_){}record.publishedMounted=false;}}records.clear();targets.clear();simpleReceiver=null;rootRecord=null;generation++;hub.clear();}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({Loader,LoaderInfo,MovieClipLoader,as2MovieMethods,loadPublish,urlForDisplay,methods,get,infoForDisplay:target=>{const info=contentInfos.get(target);if(info&&valid(infos.get(info)))return info;return target&&display.alive(target)?ensureRootInfo():null;},reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashLoader=api;
})(globalThis);
