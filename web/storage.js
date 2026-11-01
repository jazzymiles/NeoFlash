/* Original local SharedObject adapter over origin-scoped browser storage.
 * Contracts: AIR flash.net.SharedObject/ObjectEncoding/SharedObjectFlushStatus;
 * AS2 SharedObject mirror. No Flash .sol files or remote synchronization.
 * AMF payloads are real shared-codec AMF0/3 inside a versioned localStorage
 * envelope. Keys include origin, publication pathname (or an ancestor path),
 * name and secure flag. Browser HTTP and HTTPS are separate origins even when
 * secure=false. file:/opaque origins explicitly refuse persistence.
 * Explicit flush is synchronous; quota/denial throws AS3 or returns false AS2
 * with a diagnostic. No invented permission dialog or 'pending' completion.
 * pagehide flushAll is best effort, not a durability guarantee. Restart drops
 * unflushed changes/old wrappers while preserving browser storage. Concurrent
 * tabs use last successful write; no storage-event synchronization is claimed.
 * Size counts encoded AMF data bytes, not historical .sol filesystem overhead.
 */
(function(global){
  'use strict';
  const MAX_BYTES=16*1024*1024,MAX_OBJECTS=256,PREFIX='neoflash.sharedobject.v1:';
  const unsupported=name=>{throw new TypeError(`${name} is not implemented for local browser SharedObjects.`);};
  function encoding(value){if(value!==0&&value!==3)throw new RangeError('SharedObject.objectEncoding requires AMF0 (0) or AMF3 (3).');return value;}
  function constants(name,values,extra={}){const ctor=function(){unsupported(`${name} construction`);};Object.defineProperties(ctor,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>unsupported(`${name} native subclassing`)},...extra});for(const[key,value]of Object.entries(values))Object.defineProperty(ctor,key,{value,enumerable:true});return Object.freeze(ctor);}
  let dynamicWriter=null;
  const ObjectEncoding=constants('ObjectEncoding',{AMF0:0,AMF3:3,DEFAULT:3},{
    dynamicPropertyWriter:{
      enumerable:true,
      get(){return dynamicWriter;},
      set(value){
        if(value!==null&&(!value||typeof value!=='object'||typeof value.writeDynamicProperties!=='function')){
          throw new TypeError('ObjectEncoding.dynamicPropertyWriter requires null or an IDynamicPropertyWriter object with writeDynamicProperties().');
        }
        dynamicWriter=value;
      },
    },
  });
  const SharedObjectFlushStatus=constants('SharedObjectFlushStatus',{FLUSHED:'flushed',PENDING:'pending'});
  function create(options={}){
    const {amf}=options,events=options.events||global.NeoFlashEvents;
    if(!amf?.encode||!amf?.decode||!events?.EventDispatcher)throw new TypeError('SharedObject requires the shared AMF and event services.');
    const records=new Map(),states=new WeakMap(),legacyStates=new WeakMap(),token=Symbol('SharedObject');
    let generation=1,disposed=false,defaultEncoding=3;
    const hub=events.create({weakTargets:true,alive:value=>!disposed&&states.get(value)?.generation===generation});
    function open(){if(disposed)throw new Error('SharedObject service has been disposed.');}
    function state(value,legacy=false){open();const record=(legacy?legacyStates:states).get(value);if(!record||record.generation!==generation)throw new Error('This SharedObject belongs to a previous publication session.');return record;}
    function warn(error){try{if(options.onError)options.onError(error);else global.console?.warn?.(error);}catch(_){/* Best-effort legacy/pagehide diagnostics must not claim storage succeeded. */}}
    function storage(){
      try{const value=Object.hasOwn(options,'storage')?options.storage:global.localStorage;if(!value||typeof value.getItem!=='function'||typeof value.setItem!=='function'||typeof value.removeItem!=='function')throw new Error('Storage unavailable');return value;}
      catch(error){throw new Error('SharedObject browser storage is unavailable or denied. Allow storage for this origin or handle the failure.',{cause:error});}
    }
    function access(operation){try{return operation(storage());}catch(error){if(error?.message?.startsWith('SharedObject'))throw error;throw new Error('SharedObject storage operation failed: browser access or quota denied. Clear unused saved data or allow storage for this origin.',{cause:error});}}
    function location(name,localPath,secure){
      if(typeof name!=='string'||!name||name.length>1024||/[\s\x00-\x1f\x7f~%&\\;:"',<>?#]/.test(name)||name.split('/').some(piece=>piece==='.'||piece==='..'||piece===''))throw new TypeError('SharedObject name must contain 1–1024 characters without whitespace, control characters, empty/dot path components or ~%&\\;:"\',<>?#.');
      if(typeof secure!=='boolean')throw new TypeError('SharedObject secure requires a Boolean.');
      let url;try{url=new URL(options.url||global.location?.href);}catch(_){throw new Error('SharedObject requires a publication HTTP(S) URL for its origin/path scope.');}
      if(!['http:','https:'].includes(url.protocol))throw new Error('SharedObject persistence requires an HTTP(S) origin; file and opaque origins are not supported.');
      if(secure&&url.protocol!=='https:')return null;
      const publication=url.pathname;let path=publication;
      if(localPath!==null&&localPath!==undefined){
        if(typeof localPath!=='string'||localPath.length>4096||!localPath.startsWith('/')||localPath.startsWith('//')||/[\\?#\x00-\x1f\x7f]/.test(localPath))throw new TypeError('SharedObject localPath requires an absolute publication path without query/fragment.');
        const candidate=new URL(localPath,url.origin);path=candidate.pathname;
        if(path.length>1)path=path.replace(/\/+$/,'');
        if(path!=='/'&&publication!==path&&!publication.startsWith(path+'/'))throw new RangeError('SharedObject localPath must be the publication path or one of its ancestor directories.');
      }
      return PREFIX+JSON.stringify([url.origin,path,name,secure]);
    }
    function bytes(record){return amf.encode(record.data,record.encoding);}
    function unpack(text){
      if(typeof text!=='string'||text.length>Math.ceil(MAX_BYTES/3)*4+256)throw new RangeError('Saved SharedObject exceeds the 16 MiB AMF envelope limit.');
      let envelope;try{envelope=JSON.parse(text);}catch(error){throw new Error('Saved SharedObject envelope is malformed; clear this named record to recover.',{cause:error});}
      if(!envelope||envelope.version!==1||!Object.keys(envelope).every(key=>['version','encoding','bytes'].includes(key)))throw new Error('Saved SharedObject has an unsupported envelope version or fields.');
      encoding(envelope.encoding);const payload=amf.unbase64(envelope.bytes),result=amf.decode(payload,envelope.encoding,{complete:true,record:true});
      if(result.consumed!==payload.length)throw new Error('Saved SharedObject has trailing AMF data.');
      if(result.value===null||typeof result.value!=='object'||Array.isArray(result.value)||Object.getPrototypeOf(result.value)!==Object.prototype)throw new Error('Saved SharedObject root must be an ordinary data record.');
      return {data:result.value,encoding:envelope.encoding};
    }
    function getLocal(name,localPath=null,secure=false,as2=false){
      open();const key=location(name,localPath,secure);if(key===null)return null;
      let record=records.get(key);
      if(!record){
        if(records.size>=MAX_OBJECTS)throw new RangeError('At most 256 local SharedObjects may be opened in a publication session.');
        const text=access(store=>store.getItem(key)),loaded=text===null?{data:{},encoding:as2?0:defaultEncoding}:unpack(text);
        record={key,...loaded,generation,epoch:0,busy:false,modern:null,legacy:null};records.set(key,record);
      }
      if(as2){if(!record.legacy)record.legacy=new LegacySharedObject(token,record);return record.legacy;}
      if(!record.modern)record.modern=new SharedObject(token,record);return record.modern;
    }
    function flushRecord(record,minDiskSpace=0){
      if(!Number.isInteger(minDiskSpace)||minDiskSpace<0||minDiskSpace>MAX_BYTES)throw new RangeError('SharedObject.flush minDiskSpace requires an integer from 0 through 16777216; browser quota cannot be reserved.');
      if(record.busy)throw new Error('Recursive SharedObject serialization is not supported.');
      record.busy=true;const epoch=record.epoch,session=generation;
      try{
        const data=bytes(record);if(data.length>MAX_BYTES)throw new RangeError('SharedObject AMF data exceeds 16 MiB.');
        if(disposed||generation!==session||record.generation!==generation||record.epoch!==epoch)throw new Error('SharedObject changed or was reset during serialization.');
        if(Object.keys(record.data).length===0)access(store=>store.removeItem(record.key));
        else access(store=>store.setItem(record.key,JSON.stringify({version:1,encoding:record.encoding,bytes:amf.base64(data)})));
        return 'flushed';
      }finally{record.busy=false;}
    }
    function clearRecord(record,as2){
      if(as2){for(const key of Reflect.ownKeys(record.data))if(!Object.getOwnPropertyDescriptor(record.data,key).configurable)throw new TypeError('Cannot clear AS2 SharedObject.data with nonconfigurable fields.');}
      // Remove persisted data first: failed browser deletion must not discard
      // the live user's data or pretend that the record was cleared.
      access(store=>store.removeItem(record.key));record.epoch++;
      if(as2){for(const key of Reflect.ownKeys(record.data))delete record.data[key];}else record.data={};
    }
    class SharedObject extends events.EventDispatcher {
      constructor(secret,record){super();if(secret!==token)unsupported('SharedObject construction; use getLocal');states.set(this,record);Object.assign(this,hub.methods(this));Object.seal(this);}
      static getLocal(name,localPath=null,secure=false){return getLocal(name,localPath,secure,false);}
      static get defaultObjectEncoding(){open();return defaultEncoding;}
      static set defaultObjectEncoding(value){open();defaultEncoding=encoding(value);}
      get data(){return state(this).data;}
      get size(){return bytes(state(this)).length;}
      get objectEncoding(){return state(this).encoding;}
      set objectEncoding(value){const record=state(this);record.encoding=encoding(value);record.epoch++;}
      flush(minDiskSpace=0){return flushRecord(state(this),minDiskSpace);}
      clear(){clearRecord(state(this),false);}
      setProperty(name,value){const record=state(this);if(typeof name!=='string')throw new TypeError('SharedObject.setProperty name requires a string.');Object.defineProperty(record.data,name,{value,writable:true,enumerable:true,configurable:true});record.epoch++;}
      close(){flushRecord(state(this));}
    }
    class LegacySharedObject {
      constructor(secret,record){if(secret!==token)unsupported('AS2 SharedObject construction; use getLocal');legacyStates.set(this,record);}
      static getLocal(name,localPath=null,secure=false){try{return getLocal(name,localPath,secure,true);}catch(error){warn(error);return null;}}
      get data(){return state(this,true).data;}
      getSize(){return bytes(state(this,true)).length;}
      flush(minDiskSpace=0){const record=state(this,true);try{flushRecord(record,minDiskSpace);return true;}catch(error){warn(error);return false;}}
      clear(){clearRecord(state(this,true),true);}
      close(){this.flush();}
    }
    for(const[Constructor,table]of [[SharedObject,states],[LegacySharedObject,legacyStates]]){
      Object.defineProperties(Constructor,{__nfIs:{value:value=>table.has(value)},__nfInitialize:{value:()=>unsupported('SharedObject native subclassing')},getRemote:{value:()=>unsupported('SharedObject.getRemote')}});
      for(const name of ['connect','send','setDirty','setFps'])Object.defineProperty(Constructor.prototype,name,{value:()=>unsupported(`SharedObject.${name}`)});
    }
    for(const name of ['client','fps'])Object.defineProperty(SharedObject.prototype,name,{get(){state(this);unsupported(`SharedObject.${name}`);},set(){state(this);unsupported(`SharedObject.${name}`);}});
    function flushAll(){open();const failures=[];for(const record of records.values()){try{flushRecord(record);}catch(error){failures.push(error);warn(error);}}return failures;}
    function reset(){open();generation++;records.clear();hub.clear();defaultEncoding=3;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({SharedObject,LegacySharedObject,ObjectEncoding,SharedObjectFlushStatus,flushAll,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashStorage=api;
})(globalThis);
