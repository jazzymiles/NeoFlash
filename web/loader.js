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
        rawBytes:isRoot?defaultBytes:new Uint8Array(0),
        parameters:{},sharedEvents:!as2?new events.EventDispatcher():null,
        uncaughtErrorEvents:!as2?createUncaughtErrorEvents():null,
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
      get actionScriptVersion(){infoState(this);return 3;}
      get swfVersion(){infoState(this);return 12;}
      get frameRate(){infoState(this);return Number(options.getFrameRate?.())||24;}
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
    function contentType(bytes){
      if(bytes.length>=8&&[137,80,78,71,13,10,26,10].every((v,i)=>bytes[i]===v))return 'image/png';
      if(bytes.length>=3&&bytes[0]===255&&bytes[1]===216&&bytes[2]===255)return 'image/jpeg';
      throw new TypeError('Loader supports complete PNG and JPEG images. SWF, GIF and other file formats are not implemented.');
    }
    function applyContext(record,context){
      if(context===undefined||context===null)return;
      if(typeof context!=='object')throw new TypeError('LoaderContext must be an object or null.');
      if(context.applicationDomain)record.applicationDomain=context.applicationDomain;
      if(context.parameters&&typeof context.parameters==='object')record.parameters={...context.parameters};
    }
    function decode(op,bytes){
      if(!current(op))return;let pixel=null,committed=false;
      try{
        const mime=contentType(bytes);pixel=JSON.parse(player.pixel_decode(bytes));
        const dimensions=JSON.parse(player.bitmap_info(key(pixel))),record=op.record;
        record.bytesLoaded=record.bytesTotal=bytes.length;record.rawBytes=new Uint8Array(bytes);
        // Legacy completion precedes initialization. Callback replacement or
        // cancellation must not let the old decode overwrite a newer operation.
        if(record.as2&&!legacyNotify(op,'onLoadComplete',[record.target,op.status]))return;
        if(!current(op))return;
        const packet=JSON.parse(player.loader_replace(key(record.ref),key(pixel),record.as2));committed=true;
        record.pixel=pixel;record.content=packet.content_ref?proxyForRef(packet.content_ref):null;
        record.width=dimensions.width;record.height=dimensions.height;record.contentType=mime;
        if(record.content&&!record.as2)contentInfos.set(record.content,record.info);
        guarded(()=>options.onMutation?.(packet.events));if(!current(op))return;
        if(record.as2)legacyNotify(op,'onLoadInit',[record.target]);
        else if(notify(op,new events.Event('init')))notify(op,new events.Event('complete'));
        if(current(op))record.operation=null;
      }catch(error){
        if(error instanceof CallbackFailure)throw error.cause;
        if(current(op)){try{failLoad(op,error instanceof Error?error.message:String(error));}catch(callbackError){throw callbackError instanceof CallbackFailure?callbackError.cause:callbackError;}}
      }finally{if(pixel&&!committed){try{player.pixel_release(key(pixel));}catch(_){/* Restart already retired the arena. */}}}
    }
    function load(record,request,context=null){
      applyContext(record,context);
      if(!net.URLRequest.__nfIs(request))throw new TypeError('Loader.load requires a URLRequest.');
      // The transport snapshots URLRequest before async work. Its validation is
      // synchronous; use the actual request string for metadata, never fetch it.
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
    function unload(record){
      cancel(record);const hadContent=record.content!==null;
      const packet=JSON.parse(player.loader_replace(key(record.ref),'null',record.as2));
      record.content=null;record.pixel=null;record.width=record.height=record.bytesLoaded=record.bytesTotal=0;record.contentType=null;record.rawBytes=new Uint8Array(0);
      const op=begin(record,'');
      try{guarded(()=>options.onMutation?.(packet.events));if(!record.as2&&hadContent&&current(op))notify(op,new events.Event('unload'));}
      catch(error){throw error instanceof CallbackFailure?error.cause:error;}
      finally{if(current(op))record.operation=null;}
    }
    function methods(target){return {
      load:(request,context=null)=>load(state(target),request,context),loadBytes:(bytes,context=null)=>loadBytes(state(target),bytes,context),
      close:()=>cancel(state(target)),unload:()=>unload(state(target)),unloadAndStop:(gc=true)=>{if(typeof gc!=='boolean')throw new TypeError('Loader.unloadAndStop gc requires a Boolean.');unload(state(target));},
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
      const record=targetRecord(simpleLoader(),target,false);
      if(!record)throw new Error('MovieClip byte counts are available only for an actual image load; authored SWF byte counts are unavailable.');
      return record;
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
      unloadMovie(){unload(targetRecord(simpleLoader(),target,true));},
      getBytesLoaded(){return trackedMovie(target).bytesLoaded;},
      getBytesTotal(){return trackedMovie(target).bytesTotal;},
    };}
    function reset(){open();for(const record of records)cancel(record);records.clear();targets.clear();simpleReceiver=null;rootRecord=null;generation++;hub.clear();}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({Loader,LoaderInfo,MovieClipLoader,as2MovieMethods,methods,get,infoForDisplay:target=>{const info=contentInfos.get(target);if(info&&valid(infos.get(info)))return info;return target&&display.alive(target)?ensureRootInfo():null;},reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashLoader=api;
})(globalThis);
