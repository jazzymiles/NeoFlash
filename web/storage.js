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
    amf.setDynamicPropertyWriterGetter?.(()=>dynamicWriter);
    const records=new Map(),remoteRecords=new Map(),states=new WeakMap(),legacyStates=new WeakMap(),token=Symbol('SharedObject');
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
    function bytes(record){return amf.encode(record.data,record.encoding,{ignoreRootDynamicWriter:true});}
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
        record={key,name,...loaded,generation,epoch:0,busy:false,modern:null,legacy:null,peers:new Set(),clients:new WeakMap()};records.set(key,record);
      }
      if(as2){if(!record.legacy)record.legacy=new LegacySharedObject(token,record);return record.legacy;}
      if(!record.modern)record.modern=new SharedObject(token,record);return record.modern;
    }
    const channelName=options.channelName||'neoflash.sharedobject.sync.v1';
    let syncChannel=null;
    const BroadcastChannelCtor=options.BroadcastChannel||global.BroadcastChannel;
    if(typeof BroadcastChannelCtor==='function'){
      try{
        syncChannel=new BroadcastChannelCtor(channelName);
        syncChannel.onmessage=ev=>handleRemoteSyncMessage(ev?.data);
        syncChannel.unref?.();
      }catch(_){syncChannel=null;}
    }
    function makeSyncEvent(changeList){
      const ev=new events.Event('sync',false,false);
      ev.changeList=Array.isArray(changeList)?changeList:[];
      return ev;
    }
    function broadcastSync(record,changeList,handlerCall=null){
      if(!syncChannel)return;
      try{
        const encoded=record.data&&Object.keys(record.data).length?amf.base64(bytes(record)):null;
        syncChannel.postMessage({
          generation,
          key:record.key,
          name:record.name||'',
          encoding:record.encoding,
          bytes:encoded,
          changeList,
          handlerCall,
        });
      }catch(_){/* Cross-tab broadcast is best-effort */}
    }
    function handleRemoteSyncMessage(msg){
      if(disposed||!msg||typeof msg!=='object')return;
      const rec=records.get(msg.key)||remoteRecords.get(msg.key);
      if(!rec)return;
      if(typeof msg.bytes==='string'&&msg.bytes.length>0){
        try{
          const payload=amf.unbase64(msg.bytes);
          const decoded=amf.decode(payload,msg.encoding??rec.encoding,{complete:true,record:true});
          if(decoded?.value&&typeof decoded.value==='object'){
            for(const k of Object.keys(rec.data))delete rec.data[k];
            Object.assign(rec.data,decoded.value);
            rec.epoch++;
          }
        }catch(err){warn(err);}
      }else if(msg.bytes===null&&Array.isArray(msg.changeList)&&msg.changeList.some(c=>c?.code==='clear')){
        for(const k of Object.keys(rec.data))delete rec.data[k];
        rec.epoch++;
      }
      const changes=Array.isArray(msg.changeList)?msg.changeList:[{code:'change'}];
      if(rec.modern)hub.dispatch(rec.modern,makeSyncEvent(changes));
      if(rec.peers){
        for(const peer of rec.peers){
          if(SharedObject.__nfIs(peer))hub.dispatch(peer,makeSyncEvent(changes));
          else if(typeof peer.onSync==='function'){try{Reflect.apply(peer.onSync,peer,[changes]);}catch(err){warn(err);}}
        }
      }
      if(rec.legacy&&typeof rec.legacy.onSync==='function'){
        try{Reflect.apply(rec.legacy.onSync,rec.legacy,[changes]);}catch(err){warn(err);}
      }
      if(msg.handlerCall&&typeof msg.handlerCall.name==='string'){
        const {name:fnName,args=[]}=msg.handlerCall;
        if(rec.peers){
          for(const peer of rec.peers){
            const clientObj=rec.clients?.get(peer)||peer;
            const fn=clientObj?.[fnName];
            if(typeof fn==='function'){try{Reflect.apply(fn,clientObj,args);}catch(err){warn(err);}}
          }
        }
      }
    }
    function exportRecordSol(record){
      if(typeof amf.encodeSol!=='function')throw new Error('AMF service does not support .sol encoding.');
      return amf.encodeSol(record.name||'SharedObject',record.data,record.encoding,{ignoreRootDynamicWriter:true});
    }
    function importRecordSol(record,solBytes,as2=false){
      if(typeof amf.decodeSol!=='function')throw new Error('AMF service does not support .sol decoding.');
      const decoded=amf.decodeSol(solBytes);
      record.encoding=encoding(decoded.encoding);
      record.epoch++;
      if(as2){
        for(const key of Reflect.ownKeys(record.data))delete record.data[key];
        Object.assign(record.data,decoded.value);
      }else{
        record.data=decoded.value;
      }
      return decoded;
    }
    function flushRecord(record,minDiskSpace=0){
      if(!Number.isInteger(minDiskSpace)||minDiskSpace<0||minDiskSpace>MAX_BYTES)throw new RangeError('SharedObject.flush minDiskSpace requires an integer from 0 through 16777216; browser quota cannot be reserved.');
      if(record.busy)throw new Error('Recursive SharedObject serialization is not supported.');
      record.busy=true;const epoch=record.epoch,session=generation;
      try{
        const data=bytes(record);if(data.length>MAX_BYTES)throw new RangeError('SharedObject AMF data exceeds 16 MiB.');
        if(disposed||generation!==session||record.generation!==generation||record.epoch!==epoch)throw new Error('SharedObject changed or was reset during serialization.');
        if(!record.remote){
          if(Object.keys(record.data).length===0)access(store=>store.removeItem(record.key));
          else access(store=>store.setItem(record.key,JSON.stringify({version:1,encoding:record.encoding,bytes:amf.base64(data)})));
        }
        broadcastSync(record,Object.keys(record.data).map(k=>({code:'change',name:k})));
        return 'flushed';
      }finally{record.busy=false;}
    }
    function clearRecord(record,as2){
      if(as2){for(const key of Reflect.ownKeys(record.data))if(!Object.getOwnPropertyDescriptor(record.data,key).configurable)throw new TypeError('Cannot clear AS2 SharedObject.data with nonconfigurable fields.');}
      // Remove persisted data first: failed browser deletion must not discard
      // the live user's data or pretend that the record was cleared.
      if(!record.remote)access(store=>store.removeItem(record.key));
      record.epoch++;
      if(as2){for(const key of Reflect.ownKeys(record.data))delete record.data[key];}else record.data={};
      broadcastSync(record,[{code:'clear'}]);
    }
    class SharedObject extends events.EventDispatcher {
      constructor(secret,record){super();if(secret!==token)unsupported('SharedObject construction; use getLocal');states.set(this,record);Object.assign(this,hub.methods(this));Object.seal(this);}
      static getLocal(name,localPath=null,secure=false){return getLocal(name,localPath,secure,false);}
      static fromSolBytes(solBytes,localPath=null,secure=false){
        open();
        const decoded=amf.decodeSol(solBytes);
        const so=getLocal(decoded.name,localPath,secure,false);
        if(!so)return null;
        const record=state(so);
        record.encoding=encoding(decoded.encoding);
        record.data=decoded.value;
        record.epoch++;
        return so;
      }
      static get defaultObjectEncoding(){open();return defaultEncoding;}
      static set defaultObjectEncoding(value){open();defaultEncoding=encoding(value);}
      get data(){return state(this).data;}
      get size(){return bytes(state(this)).length;}
      get objectEncoding(){return state(this).encoding;}
      set objectEncoding(value){const record=state(this);record.encoding=encoding(value);record.epoch++;}
      flush(minDiskSpace=0){return flushRecord(state(this),minDiskSpace);}
      clear(){clearRecord(state(this),false);}
      setProperty(name,value){
        const record=state(this);
        if(typeof name!=='string')throw new TypeError('SharedObject.setProperty name requires a string.');
        const oldValue=record.data[name];
        if(value===undefined)delete record.data[name];
        else Object.defineProperty(record.data,name,{value,writable:true,enumerable:true,configurable:true});
        record.epoch++;
        broadcastSync(record,[{code:'change',name,oldValue}]);
      }
      exportSolBytes(){return exportRecordSol(state(this));}
      importSolBytes(solBytes){importRecordSol(state(this),solBytes,false);return this;}
      close(){
        const record=state(this);
        if(record.remote){record.peers?.delete(this);return;}
        flushRecord(record);
      }
    }
    class LegacySharedObject {
      constructor(secret,record){if(secret!==token)unsupported('AS2 SharedObject construction; use getLocal');legacyStates.set(this,record);this.onStatus=null;this.onSync=null;}
      static getLocal(name,localPath=null,secure=false){try{return getLocal(name,localPath,secure,true);}catch(error){warn(error);return null;}}
      static fromSolBytes(solBytes,localPath=null,secure=false){
        try{
          open();
          const decoded=amf.decodeSol(solBytes);
          const so=getLocal(decoded.name,localPath,secure,true);
          if(!so)return null;
          const record=state(so,true);
          record.encoding=encoding(decoded.encoding);
          for(const key of Reflect.ownKeys(record.data))delete record.data[key];
          Object.assign(record.data,decoded.value);
          record.epoch++;
          return so;
        }catch(error){warn(error);return null;}
      }
      static getRemote(name,remotePath=null,persistence=false,secure=false){
        try{
          open();const check=location(name,null,Boolean(secure));if(check===null)return null;
          if(remotePath!==null&&remotePath!==undefined&&(typeof remotePath!=='string'||remotePath.length>4096||/[\x00-\x1f\x7f]/.test(remotePath)))throw new TypeError('SharedObject.getRemote remotePath requires a valid URI/path String or null.');
          const key=`remote:${remotePath??''}:${name}:${Boolean(secure)}`;
          let record=remoteRecords.get(key);
          if(!record){
            if(remoteRecords.size>=MAX_OBJECTS)throw new RangeError('At most 256 remote SharedObjects may be opened in a publication session.');
            record={key,name,data:{},encoding:0,generation,epoch:0,busy:false,remote:true,persistence,fps:0,peers:new Set(),clients:new WeakMap()};
            remoteRecords.set(key,record);
          }
          const instance=new LegacySharedObject(token,record);
          return instance;
        }catch(error){warn(error);return null;}
      }
      get data(){return state(this,true).data;}
      getSize(){return bytes(state(this,true)).length;}
      exportSolBytes(){return exportRecordSol(state(this,true));}
      importSolBytes(solBytes){importRecordSol(state(this,true),solBytes,true);return this;}
      flush(minDiskSpace=0){
        const record=state(this,true);
        try{
          if(record.remote){if(!Number.isInteger(minDiskSpace)||minDiskSpace<0||minDiskSpace>MAX_BYTES)throw new RangeError('Invalid minDiskSpace.');bytes(record);broadcastSync(record,Object.keys(record.data).map(k=>({code:'change',name:k})));return true;}
          flushRecord(record,minDiskSpace);return true;
        }catch(error){
          warn(error);
          const info={code:'SharedObject.Flush.Failed',level:'error',description:String(error?.message||error)};
          if(typeof this.onStatus==='function'){try{Reflect.apply(this.onStatus,this,[info]);}catch(err){warn(err);}}
          else if(typeof options.onSystemStatus==='function')options.onSystemStatus(info);
          return false;
        }
      }
      clear(){
        const record=state(this,true);
        if(record.remote){for(const key of Reflect.ownKeys(record.data))delete record.data[key];record.epoch++;broadcastSync(record,[{code:'clear'}]);return;}
        clearRecord(record,true);
      }
      connect(nc){
        const record=state(this,true);
        if(!record.remote)throw new TypeError('SharedObject.connect requires a remote SharedObject from SharedObject.getRemote.');
        if(!nc||typeof nc!=='object'||!('isConnected'in nc||'uri'in nc))return false;
        record.peers.add(this);const born=generation;
        queueMicrotask(()=>{if(disposed||born!==generation||!record.peers.has(this))return;try{if(typeof this.onSync==='function')Reflect.apply(this.onSync,this,[[{code:'clear'}]]);}catch(error){warn(error);}});
        return true;
      }
      setFps(updatesPerSecond){
        const record=state(this,true);
        if(!record.remote||typeof updatesPerSecond!=='number'||!Number.isFinite(updatesPerSecond)||updatesPerSecond<0)return false;
        record.fps=updatesPerSecond;return true;
      }
      setDirty(propertyName){
        const record=state(this,true);
        if(propertyName!==undefined&&typeof propertyName!=='string')throw new TypeError('SharedObject.setDirty propertyName requires a String.');
        record.epoch++;
        if(record.remote&&record.peers.has(this)&&typeof propertyName==='string'&&propertyName){
          const born=generation,targets=[...record.peers];
          broadcastSync(record,[{code:'change',name:propertyName}]);
          queueMicrotask(()=>{if(disposed||born!==generation)return;for(const peer of targets){if(!record.peers.has(peer))continue;try{if(typeof peer.onSync==='function')Reflect.apply(peer.onSync,peer,[[{code:'change',name:propertyName}]]);}catch(error){warn(error);}}});
        }
      }
      send(handlerName,...args){
        const record=state(this,true);
        if(!record.remote||!record.peers.has(this))throw new Error('SharedObject.send requires a connected remote SharedObject.');
        if(typeof handlerName!=='string'||!handlerName||['__proto__','prototype','constructor'].includes(handlerName))throw new TypeError('SharedObject.send requires a valid handler name String.');
        const born=generation,targets=[...record.peers];
        broadcastSync(record,[],{name:handlerName,args});
        queueMicrotask(()=>{if(disposed||born!==generation)return;for(const peer of targets){if(!record.peers.has(peer))continue;try{const fn=peer[handlerName];if(typeof fn==='function')Reflect.apply(fn,peer,args);}catch(error){warn(error);}}});
      }
      close(){
        const record=state(this,true);
        if(record.remote){record.peers.delete(this);return;}
        this.flush();
      }
    }
    for(const[Constructor,table]of [[SharedObject,states],[LegacySharedObject,legacyStates]]){
      Object.defineProperties(Constructor,{__nfIs:{value:value=>table.has(value)},__nfInitialize:{value:()=>unsupported('SharedObject native subclassing')}});
    }
    function createRemoteModern(name,remotePath=null,persistence=false,secure=false){
      open();
      if(typeof name!=='string'||!name||name.length>1024||/[\s\x00-\x1f\x7f~%&\\;:"',<>?#]/.test(name))throw new TypeError('SharedObject.getRemote requires a valid name String.');
      if(typeof secure!=='boolean')throw new TypeError('SharedObject secure requires a Boolean.');
      if(remotePath!==null&&remotePath!==undefined&&(typeof remotePath!=='string'||remotePath.length>4096||/[\x00-\x1f\x7f]/.test(remotePath)))throw new TypeError('SharedObject.getRemote remotePath requires a valid URI/path String or null.');
      if(secure){
        let url;try{url=new URL(options.url||global.location?.href);}catch(_){url=null;}
        if(url&&url.protocol==='http:')return null;
      }
      const key=`as3remote:${remotePath??''}:${name}:${secure}`;
      let record=remoteRecords.get(key);
      if(!record){
        if(remoteRecords.size>=MAX_OBJECTS)throw new RangeError('At most 256 remote SharedObjects may be opened in a publication session.');
        record={key,name,remotePath,data:{},encoding:defaultEncoding,generation,epoch:0,busy:false,remote:true,persistence,secure,fps:0,peers:new Set(),dirty:new Set(),clients:new WeakMap()};
        remoteRecords.set(key,record);
      }
      const instance=new SharedObject(token,record);
      record.clients.set(instance,instance);
      return instance;
    }
    Object.defineProperty(SharedObject,'getRemote',{configurable:true,writable:true,value:createRemoteModern});
    Object.defineProperties(SharedObject.prototype,{
      client:{
        configurable:true,
        enumerable:true,
        get(){const rec=state(this);return rec.clients?.get(this)||this;},
        set(v){
          const rec=state(this);
          if(v===null||v===undefined||(typeof v!=='object'&&typeof v!=='function'))throw new TypeError('TypeError: Error #2007: SharedObject.client must be a non-null Object.');
          if(!rec.clients)rec.clients=new WeakMap();
          rec.clients.set(this,v);
        },
      },
      fps:{
        configurable:true,
        enumerable:true,
        get(){return Number(state(this).fps||0);},
        set(v){const rec=state(this);const n=Number(v);if(!Number.isFinite(n)||n<0)throw new RangeError('SharedObject.fps must be >= 0.');rec.fps=n;},
      },
      connect:{
        configurable:true,
        writable:true,
        value:function connect(myConnection,params=null){
          const rec=state(this);
          if(!myConnection||typeof myConnection!=='object')throw new TypeError('TypeError: Error #2007: SharedObject.connect requires a valid NetConnection.');
          rec.connectParams=params;
          if(!rec.peers)rec.peers=new Set();
          rec.peers.add(this);
          const born=generation;
          queueMicrotask(()=>{
            if(disposed||generation!==born||!rec.peers.has(this))return;
            const changes=Object.keys(rec.data).map(k=>({code:'change',name:k}));
            if(changes.length===0)changes.push({code:'clear'});
            hub.dispatch(this,makeSyncEvent(changes));
          });
        },
      },
      setDirty:{
        configurable:true,
        writable:true,
        value:function setDirty(propertyName){
          const rec=state(this);
          if(propertyName===null||propertyName===undefined)throw new TypeError('TypeError: Error #2007: SharedObject.setDirty propertyName must be non-null.');
          const key=String(propertyName);
          rec.epoch++;
          if(!rec.dirty)rec.dirty=new Set();
          rec.dirty.add(key);
          broadcastSync(rec,[{code:'change',name:key}]);
          if(rec.remote&&rec.peers?.size){
            const born=generation,targets=[...rec.peers];
            queueMicrotask(()=>{
              if(disposed||generation!==born)return;
              for(const peer of targets)if(rec.peers.has(peer))hub.dispatch(peer,makeSyncEvent([{code:'change',name:key}]));
            });
          }
        },
      },
      setFps:{
        configurable:true,
        writable:true,
        value:function setFps(updatesPerSecond){
          const rec=state(this);
          const n=Number(updatesPerSecond);
          if(!Number.isFinite(n)||n<0)return false;
          rec.fps=n;return true;
        },
      },
      send:{
        configurable:true,
        writable:true,
        value:function send(handlerName,...args){
          const rec=state(this);
          if(typeof handlerName!=='string'||!handlerName)throw new TypeError('SharedObject.send requires a valid handler name String.');
          broadcastSync(rec,[],{name:handlerName,args});
          const targets=rec.peers?.size?[...rec.peers]:[this];
          const born=generation;
          queueMicrotask(()=>{
            if(disposed||generation!==born)return;
            for(const peer of targets){
              const clientObj=rec.clients?.get(peer)||peer;
              const fn=clientObj?.[handlerName];
              if(typeof fn==='function'){try{Reflect.apply(fn,clientObj,args);}catch(err){warn(err);}}
            }
          });
        },
      },
    });
    const _as3Hook=Object.freeze({createRemoteModern,state:v=>state(v,false),bytes,encoding,hub,events,warn,MAX_BYTES,isDisposed:()=>disposed,getGeneration:()=>generation,broadcastSync,handleRemoteSyncMessage});
    function flushAll(){open();const failures=[];for(const record of records.values()){try{flushRecord(record);}catch(error){failures.push(error);warn(error);}}return failures;}
    function reset(){open();generation++;records.clear();remoteRecords.clear();hub.clear();defaultEncoding=3;dynamicWriter=null;}
    function dispose(){if(disposed)return;reset();try{syncChannel?.close?.();}catch(_){}syncChannel=null;disposed=true;}
    return Object.freeze({SharedObject,LegacySharedObject,ObjectEncoding,SharedObjectFlushStatus,flushAll,reset,dispose,_as3Hook});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashStorage=api;
})(globalThis);
