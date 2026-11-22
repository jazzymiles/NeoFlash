/* Original local SharedObject adapter over origin-scoped browser storage.
 * Contracts: AIR flash.net.SharedObject/ObjectEncoding/SharedObjectFlushStatus;
 * AS2 SharedObject mirror. Explicit .sol exchange uses the shared native codec;
 * browser persistence remains a separate, versioned storage envelope.
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
  // A socket owns its subscriptions. Each peer receives the relay's snapshot and
  // revision-ordered patches even if two publications do not share an origin.
  function createNetwork(amf, cacheIO, notify, status, invoke) {
    const sockets = new Map(), peers = new WeakMap();
    let serial = 0, resolveConnection = () => null;
    const validName = name => typeof name === 'string' && name.length > 0 &&
      name.length <= 1024 && !/[\u0000-\u001f\u007f]/.test(name) && !['__proto__','constructor','prototype'].includes(name);
    const isNetwork = rec => rec.usedSocket || /^wss?:/i.test(rec.remotePath || '');
    function pack(value, encoding) {
      const bytes = amf.encode(value, encoding);
      if (bytes.length > 192 * 1024) throw new RangeError('Remote SharedObject value exceeds the relay field limit.');
      return {encoding, bytes: amf.base64(bytes)};
    }
    function unpack(value) {
      if (!value || ![0,3].includes(value.encoding) || typeof value.bytes !== 'string' || value.bytes.length > 256 * 1024)
        throw new TypeError('Invalid SharedObject AMF field.');
      return amf.decode(amf.unbase64(value.bytes), value.encoding, {complete:true}).value;
    }
    // The disk copy contains the last acknowledged state and a per-field outbox.
    // A write remains in the outbox until an authoritative response confirms it.
    function openCache(rec,endpoint) {
      if (typeof rec.persistence !== 'string' || !/^wss?:/i.test(endpoint || '')) return;
      const io = cacheIO(rec,endpoint);
      if (rec.cache) {
        if (rec.cache.key !== io.key) throw new Error('Specify the endpoint when opening a locally persistent SharedObject for a different server.');
        return;
      }
      const text = io.read();
      if (text !== null && text.length > 1024 * 1024) throw new Error('Stored remote SharedObject exceeds its size limit.');
      const saved = text === null ? {version:1,fields:{},versions:{},dirty:{}} : JSON.parse(text);
      const entries = value => {
        if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid remote SharedObject cache.');
        const items = Object.entries(value);
        if (items.length > 1024 || items.some(([key])=>!validName(key))) throw new Error('Invalid remote SharedObject cache keys.');
        return new Map(items);
      };
      if (saved.version !== 1) throw new Error('Unknown remote SharedObject cache version.');
      const cache = {...io,text,fields:entries(saved.fields),versions:entries(saved.versions),dirty:entries(saved.dirty)};
      const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
      if (cache.fields.size !== cache.versions.size || [...cache.fields.keys()].some(k=>!hash(cache.versions.get(k)))) throw new Error('Invalid remote SharedObject cache versions.');
      const live = new Map();
      for (const [key,wire] of cache.fields) live.set(key,unpack(wire));
      for (const [key,edit] of cache.dirty) {
        if (!edit || (edit.from !== null && !hash(edit.from))) throw new Error('Invalid remote SharedObject offline edit.');
        if (edit.value === null) live.delete(key);
        else live.set(key,unpack(edit.value));
      }
      for (const [key,value] of live) Object.defineProperty(rec.data,key,{value,writable:true,enumerable:true,configurable:true});
      rec.cache = cache;
    }
    const sameWire = (a,b) => a===b || Boolean(a && b && a.encoding===b.encoding && a.bytes===b.bytes);
    const sameEdit = (a,b) => a===b || Boolean(a && b && a.from===b.from && sameWire(a.value,b.value));
    function mergeCacheMap(previous, proposed, entries, equal, authored) {
      const stored = new Map(entries);
      const merged = new Map(stored);
      for (const key of new Set([...previous.keys(),...proposed.keys()])) {
        const before = previous.get(key), next = proposed.get(key);
        if (equal(before,next)) continue;
        // A stale acknowledgement must never discard an edit made by another
        // publication sharing the same local cache. A new authored edit wins.
        if (!equal(before,stored.get(key)) && !(authored && proposed.has(key))) continue;
        if (proposed.has(key)) merged.set(key,next);
        else merged.delete(key);
      }
      return merged;
    }
    function save(cache,fields,versions,dirty,authored=false) {
      const stored = cache.read();
      if (stored !== cache.text) {
        if(stored && stored.length>1024*1024)throw new Error('Remote SharedObject cache exceeds 1 MiB.');
        const latest = stored ? JSON.parse(stored) : {version:1,fields:{},versions:{},dirty:{}};
        if(latest.version!==1)throw new Error('Unknown remote SharedObject cache version.');
        fields=mergeCacheMap(cache.fields,fields,Object.entries(latest.fields),sameWire,false);
        versions=mergeCacheMap(cache.versions,versions,Object.entries(latest.versions),Object.is,false);
        dirty=mergeCacheMap(cache.dirty,dirty,Object.entries(latest.dirty),sameEdit,authored);
      }
      if (fields.size > 1024 || dirty.size > 1024) throw new Error('Remote SharedObject cache field limit reached.');
      const text = fields.size || dirty.size ? JSON.stringify({version:1,fields:Object.fromEntries(fields),versions:Object.fromEntries(versions),dirty:Object.fromEntries(dirty)}) : null;
      if (text && text.length > 1024 * 1024) throw new Error('Remote SharedObject cache exceeds 1 MiB.');
      if (stored !== text) cache.write(text);
      Object.assign(cache,{text,fields,versions,dirty});
    }
    function remember(rec,key,value) {
      const c = rec.cache;
      if (!c) return;
      const dirty = new Map(c.dirty);
      const previous = dirty.get(key);
      dirty.set(key,{from:previous ? previous.from : c.versions.get(key) ?? null,value});
      save(c,c.fields,c.versions,dirty,true);
    }
    function sendPatch(s,fields,expected) {
      if (s.sent.length >= 64) throw new Error('Too many pending SharedObject acknowledgements.');
      write(s,'patch',{fields:Object.fromEntries(fields),...(expected ? {expected:Object.fromEntries(expected)} : {})});
      s.sent.push(new Map(fields));
    }
    function replay(s,cache) {
      if (!cache?.dirty.size) return;
      const fields = new Map(), expected = new Map();
      for (const [name,edit] of cache.dirty) { fields.set(name,edit.value); expected.set(name,edit.from); }
      sendPatch(s,fields,expected);
    }
    function reconcile(rec, msg, acknowledged) {
      if (msg.op!=='clear' && (!msg.versions || typeof msg.versions!=='object' || Array.isArray(msg.versions))) throw new Error('Update the SharedObject relay to enable locally persistent rooms.');
      const c = rec.cache;
      const fields = msg.op==='patch' ? new Map(c.fields) : new Map();
      const versions = msg.op==='patch' ? new Map(c.versions) : new Map();
      const dirty = msg.op==='clear' && msg.own ? new Map() : new Map(c.dirty);
      for (const [name,wire] of Object.entries(msg.fields || {})) {
        if (wire === null) { fields.delete(name); versions.delete(name); }
        else {
          const version = msg.versions?.[name];
          if (typeof version !== 'string' || !/^[a-f0-9]{64}$/.test(version)) throw new Error('The relay does not provide offline reconciliation versions. Update neoflash-sync.');
          fields.set(name,wire); versions.set(name,version);
        }
        if (acknowledged?.has(name) && dirty.has(name)) {
          const wanted = dirty.get(name).value, sent = acknowledged.get(name);
          if (wanted?.encoding === sent?.encoding && wanted?.bytes === sent?.bytes) dirty.delete(name);
          else dirty.set(name,{from:versions.get(name) ?? null,value:wanted});
        }
      }
      save(c,fields,versions,dirty);
    }
    function write(s, op, details = {}) {
      if (s.socket.readyState !== 1) throw new Error('Remote SharedObject connection is closed.');
      if (s.socket.bufferedAmount > 1024 * 1024) throw new Error('Remote SharedObject send queue is full.');
      const wire = JSON.stringify({type:'so', op, room:s.room, sub:s.sub, ...details});
      if (new TextEncoder().encode(wire).length > 512 * 1024) throw new RangeError('Remote SharedObject message exceeds 512 KiB.');
      s.socket.send(wire);
    }
    function stop(s, reason) {
      if (s.socket.readyState === 1) { try { write(s,'leave'); } catch (_) {} }
      clearTimeout(s.timer);
      clearTimeout(s.joinTimer);
      sockets.get(s.socket)?.delete(s.room);
      if (!sockets.get(s.socket)?.size) sockets.delete(s.socket);
      for (const [peer,rec] of s.peers) {
        peers.delete(peer);
        rec.network = null;
        if (reason) {
          if (s.flushing.includes(peer)) status(peer,'SharedObject.Flush.Failed',reason);
          status(peer,'SharedObject.Connect.Failed',reason);
        }
      }
      s.peers.clear();
    }
    function requirePeer(peer) {
      const s = peers.get(peer);
      if (!s) throw new Error('Connect the remote SharedObject to an open NetConnection first.');
      return s;
    }
    function drain(s) {
      clearTimeout(s.timer);
      s.timer = null;
      if (s.pending.size > 0) {
        sendPatch(s,s.pending);
        s.pending.clear();
      }
    }
    function update(peer, rec, key, value, remove) {
      const s = rec.cache ? peers.get(peer) : requirePeer(peer);
      if (!validName(key)) throw new TypeError('Invalid remote SharedObject property name.');
      const exists = Object.hasOwn(rec.data,key);
      if (remove ? !exists : exists && Object.is(rec.data[key],value)) return;
      const wire = remove ? null : pack(value,rec.encoding);
      remember(rec,key,wire);
      if (remove) delete rec.data[key];
      else Object.defineProperty(rec.data,key,{value,writable:true,configurable:true,enumerable:true});
      rec.epoch++;
      if(s && (!rec.cache || s.ready)) queue(s,rec,key,wire);
    }
    function dirty(peer, rec, key) {
      const s = rec.cache ? peers.get(peer) : requirePeer(peer);
      if (!validName(key)) throw new TypeError('Invalid remote SharedObject property name.');
      const wire = Object.hasOwn(rec.data,key) ? pack(rec.data[key],rec.encoding) : null;
      remember(rec,key,wire);
      if(s && (!rec.cache || s.ready)) queue(s,rec,key,wire);
    }
    function queue(s,rec,key,wire) {
      s.pending.set(key,wire);
      if (s.timer === null) s.timer = setTimeout(() => {
        try { drain(s); } catch (e) { stop(s,String(e)); }
      }, rec.fps > 0 ? Math.max(1, Math.round(1000 / rec.fps)) : 0);
    }
    function connect(peer, rec, nc, params) {
      const connection = resolveConnection(nc);
      if (!isNetwork(rec) && !/^wss?:/i.test(connection?.uriValue || '')) return false;
      if (params != null) throw new Error('The SharedObject relay does not accept application connection parameters.');
      if (!rec.remote) throw new Error('WebSocket synchronization requires SharedObject.getRemote.');
      const socket = connection?.ws;
      if (!connection?.connected || !socket || socket.readyState !== 1) throw new Error('Open a WebSocket NetConnection before joining a remote SharedObject.');
      if (rec.remotePath && new URL(rec.remotePath).href !== new URL(connection.uriValue).href)
        throw new Error('The SharedObject remotePath must match the NetConnection WebSocket endpoint.');
      if (rec.secure && !/^wss:/i.test(connection.uriValue)) throw new Error('Secure remote SharedObjects require wss.');
      if (typeof rec.persistence !== 'boolean' && typeof rec.persistence !== 'string') throw new Error('SharedObject persistence requires a Boolean or local path.');
      openCache(rec,connection.uriValue);
      const persistent = rec.persistence !== false;
      const previous = peers.get(peer);
      if (previous && previous.socket === socket) return true;
      if (rec.network && rec.network.socket !== socket) throw new Error('Close this named SharedObject before switching connections.');
      let rooms = sockets.get(socket);
      if (!rooms) sockets.set(socket, rooms = new Map());
      let s = rooms.get(rec.name);
      if (!s) {
        s = {socket,room:rec.name,sub:++serial,peers:new Map(),persistent,fields:new Map(),versions:new Map(),sent:[],pending:new Map(),flushing:[],clearing:[],timer:null,joinTimer:null,ready:false};
        rooms.set(rec.name,s);
        try { write(s,'join',{persistent}); }
        catch (e) { stop(s); throw e; }
        s.joinTimer = setTimeout(() => stop(s,'The server did not acknowledge the NeoFlash SharedObject subscription.'),10000);
      }
      if (s.persistent !== persistent) throw new Error('Room subscribers must use the same persistence setting.');
      const first = s.peers.values().next().value;
      rec.usedSocket = true;
      rec.network = s;
      s.peers.set(peer,rec);
      peers.set(peer,s);
      if (s.ready && first) {
        if (first !== rec) {
          const snapshot = {op:'snapshot',fields:Object.fromEntries(s.fields),versions:Object.fromEntries(s.versions)};
          applyRecord(s,rec,snapshot,decodeChanges(snapshot),null,new Set());
        }
        queueMicrotask(() => { if (peers.get(peer) === s) notify(peer,[{code:'clear'},...Object.keys(rec.data).map(name=>({code:'change',name}))]); });
      }
      return true;
    }
    function decodeChanges(msg) {
      return Object.entries(msg.fields || {}).map(([key,wire]) => {
        if (!validName(key)) throw new Error('Invalid remote SharedObject property.');
        return [key,wire === null,wire === null ? undefined : unpack(wire)];
      });
    }
    function applyRecord(s,rec,msg,changes,acknowledged,rejected) {
      const list = [];
      if (rec.cache) reconcile(rec,msg,acknowledged);
      if (msg.op !== 'patch') {
        list.push({code:'clear'});
        for (const key of Object.keys(rec.data)) delete rec.data[key];
      }
      for (const [name,remove,value] of changes) {
        const oldValue = rec.data[name];
        if (remove) delete rec.data[name];
        else Object.defineProperty(rec.data,name,{value,writable:true,configurable:true,enumerable:true});
        const code = rejected.has(name) ? 'reject' : msg.own ? 'success' : remove ? 'delete' : 'change';
        list.push({code,name,oldValue:code === 'reject' || code === 'change' ? oldValue : null});
      }
      if (rec.cache) {
        const dirty = rec.cache.dirty;
        for (const [name,edit] of dirty) {
          if (msg.op==='patch' && !Object.hasOwn(msg.fields,name)) continue;
          if (edit.value === null) delete rec.data[name];
          else Object.defineProperty(rec.data,name,{value:unpack(edit.value),writable:true,enumerable:true,configurable:true});
        }
        if (msg.op === 'snapshot') replay(s,rec.cache);
      }
      rec.epoch++;
      return list;
    }
    function receive(socket, msg) {
      const s = sockets.get(socket)?.get(msg.room);
      if (!s || s.sub !== msg.sub) return;
      try {
        if (msg.op === 'error') { stop(s,String(msg.description)); return; }
        if (msg.op === 'flushed') {
          const peer = s.flushing.shift();
          if (peer && s.peers.has(peer)) status(peer,'SharedObject.Flush.Success');
          return;
        }
        if (msg.op === 'call') {
          if (!validName(msg.method)) throw new Error('Invalid SharedObject callback.');
          const args = unpack(msg.args);
          if (!Array.isArray(args) || args.length > 256) throw new Error('Invalid SharedObject callback arguments.');
          for (const [peer, rec] of s.peers) invoke(peer,rec,msg.method,args);
          return;
        }
        if (!['snapshot','clear','patch'].includes(msg.op)) throw new Error('Unknown SharedObject relay operation.');
        if (msg.op !== 'clear' && (!msg.fields || typeof msg.fields !== 'object' || Array.isArray(msg.fields))) throw new Error('Invalid remote SharedObject fields.');
        const changes = decodeChanges(msg);
        if (msg.op !== 'patch') { s.fields.clear(); s.versions.clear(); }
        for (const [name,wire] of Object.entries(msg.fields || {})) {
          if (wire === null) { s.fields.delete(name); s.versions.delete(name); }
          else { s.fields.set(name,wire); if(msg.versions?.[name])s.versions.set(name,msg.versions[name]); }
        }
        const delivered = new Map();
        const acknowledged = msg.op === 'patch' && msg.own ? s.sent.shift() : null;
        const rejected = new Set(msg.rejected || []);
        for (const rec of new Set(s.peers.values())) {
          const list = applyRecord(s,rec,msg,changes,acknowledged,rejected);
          delivered.set(rec,list);
        }
        if (msg.op === 'snapshot') { clearTimeout(s.joinTimer); s.ready = true; }
        const cleared = msg.op === 'clear' && msg.own ? s.clearing.shift() : null;
        // The clearing subscriber disconnects after the confirmed purge.
        const recipients = [...s.peers];
        if (cleared && peers.get(cleared) === s) close(cleared);
        for (const [peer,rec] of recipients) if (peer === cleared || peers.get(peer) === s) notify(peer,delivered.get(rec));
      } catch (error) { stop(s,String(error)); }
    }
    function close(peer) {
      const s = peers.get(peer);
      if (!s) return;
      const rec = s.peers.get(peer);
      drain(s);
      peers.delete(peer);
      s.peers.delete(peer);
      if (![...s.peers.values()].includes(rec)) rec.network = null;
      if (!s.peers.size) stop(s);
    }
    function disconnect(socket, reason) {
      for (const s of [...(sockets.get(socket)?.values() || [])]) stop(s,reason);
    }
    return {
      isNetwork, openCache, connect, receive, dirty, update, close, disconnect,
      setResolver(fn) { resolveConnection = fn; },
      flush(peer,rec) {
        if (rec.cache) {
          const s = peers.get(peer);
          if (s) drain(s);
          return 'flushed';
        }
        const s = requirePeer(peer);
        if(s.flushing.length>=64)throw new Error('Too many outstanding SharedObject.flush requests.');
        drain(s); write(s,'flush'); s.flushing.push(peer);
        return 'pending';
      },
      clear(peer,rec) {
        if(rec.cache && !peers.has(peer)){
          rec.cache.write(null);
          Object.assign(rec.cache,{text:null,fields:new Map(),versions:new Map(),dirty:new Map()});
          for(const key of Object.keys(rec.data))delete rec.data[key];
          return;
        }
        const s = requirePeer(peer);
        if(s.clearing.includes(peer))return;
        drain(s); write(s,'clear'); s.clearing.push(peer);
      },
      send(peer,rec,method,args) {
        if (!validName(method) || /[./]/.test(method) || ['clear','close','connect','flush','send','setDirty','setProperty','setFps'].includes(method)) throw new Error('Invalid remote SharedObject callback name.');
        const s = requirePeer(peer); drain(s); write(s,'call',{method,args:pack(args,rec.encoding)});
      },
      reset() { for (const s of [...sockets.values()].flatMap(rooms=>[...rooms.values()])) {
        stop(s);
      } },
    };
  }
  function create(options={}){
    const {amf}=options,events=options.events||global.NeoFlashEvents;
    if(!amf?.encode||!amf?.decode||!events?.EventDispatcher)throw new TypeError('SharedObject requires the shared AMF and event services.');
    amf.setDynamicPropertyWriterGetter?.(()=>dynamicWriter);
    const records=new Map(),remoteRecords=new Map(),states=new WeakMap(),legacyStates=new WeakMap(),token=Symbol('SharedObject');
    let generation=1,disposed=false,defaultEncoding=3;
    const hub=events.create({weakTargets:true,alive:value=>!disposed&&states.get(value)?.generation===generation});
    function cacheIO(rec,endpoint){
      const site = new URL(options.url || global.location?.href);
      const path = rec.persistence;
      if (!['http:','https:'].includes(site.protocol) || !path || path.length>4096 || /[\\?#\x00-\x1f\x7f]/.test(path) || path.startsWith('//') || path.split('/').some(p=>/^\.{1,2}$/.test(p.replace(/%2e/ig,'.')))) throw new Error('Use a local SharedObject cache path within the HTTP(S) publication origin.');
      const local = new URL(path,site);
      if(local.origin!==site.origin)throw new Error('SharedObject cache must belong to the publication origin.');
      const key = 'neoflash.remote.v1:'+JSON.stringify([site.origin,local.pathname,new URL(endpoint).href,rec.name,Boolean(rec.secure)]);
      return {key,read:()=>access(store=>store.getItem(key)),write:text=>access(store=>text===null ? store.removeItem(key) : store.setItem(key,text))};
    }
    const network=createNetwork(amf,cacheIO,
      (peer,changes)=>{
        try{
          if(states.has(peer))hub.dispatch(peer,networkSync(changes));
          else if(typeof peer.onSync==='function')peer.onSync(changes);
        }catch(error){warn(error);}
      },
      (peer,code,description)=>{
        const info={code,level:description?'error':'status',description:description||''};
        try{
          if(states.has(peer)&&peer.hasEventListener('netStatus'))hub.dispatch(peer,networkStatus(info));
          else if(typeof peer.onStatus==='function')peer.onStatus(info);
          else if(description)warn(new Error(description));
        }catch(error){warn(error);}
      },
      (peer,rec,name,args)=>{
        const client=rec.clients?.get(peer)||peer;
        // Dispatch only application callbacks. Never expose inherited host methods to the wire.
        for(let obj=client;obj&&obj!==Object.prototype&&obj!==Function.prototype&&obj!==LegacySharedObject.prototype&&obj!==SharedObject.prototype;obj=Object.getPrototypeOf(obj)){
          if(client===peer&&['onStatus','onSync','addEventListener','removeEventListener','hasEventListener','willTrigger','dispatchEvent'].includes(name))return;
          const d=Object.getOwnPropertyDescriptor(obj,name);
          if(d){if(typeof d.value==='function'){try{Reflect.apply(d.value,client,args);}catch(e){warn(e);}}return;}
        }
      });
    let networkSync = makeSyncEvent, networkStatus = info => Object.assign(new events.Event("netStatus"),{info});
    function open(){if(disposed)throw new Error('SharedObject service has been disposed.');}
    function state(value,legacy=false){open();const record=(legacy?legacyStates:states).get(value);if(!record||record.generation!==generation)throw new Error('This SharedObject belongs to a previous publication session.');return record;}
    function warn(error){try{if(options.onError)options.onError(error);else global.console?.warn?.(error);}catch(_){/* Best-effort legacy/pagehide diagnostics must not claim storage succeeded. */}}
    function storage(){
      try{const value=Object.hasOwn(options,'storage')?options.storage:global.localStorage;if(!value||typeof value.getItem!=='function'||typeof value.setItem!=='function'||typeof value.removeItem!=='function')throw new Error('Storage unavailable');return value;}
      catch(error){throw new Error('SharedObject browser storage is unavailable or denied. Allow storage for this origin or handle the failure.',{cause:error});}
    }
    function access(operation){try{return operation(storage());}catch(error){if(error?.message?.startsWith('SharedObject'))throw error;throw new Error('SharedObject storage operation failed: browser access or quota denied. Clear unused saved data or allow storage for this origin.',{cause:error});}}
    function completeUnicode(value){
      for(let i=0;i<value.length;i++){const c=value.charCodeAt(i);if(c>=0xd800&&c<=0xdbff){const low=value.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))return false;}else if(c>=0xdc00&&c<=0xdfff)return false;}return true;
    }
    function location(name,localPath,secure){
      if(typeof name!=='string'||!name||name.length>1024||/[\s\x00-\x1f\x7f~%&\\;:"',<>?#]/.test(name)||name.split('/').some(piece=>piece==='.'||piece==='..'||piece===''))throw new TypeError('SharedObject name must contain 1–1024 characters without whitespace, control characters, empty/dot path components or ~%&\\;:"\',<>?#.');
      if(!completeUnicode(name))throw new TypeError('SharedObject names require complete Unicode characters.');
      if(typeof secure!=='boolean')throw new TypeError('SharedObject secure requires a Boolean.');
      let url;try{url=new URL(options.url||global.location?.href);}catch(_){throw new Error('SharedObject requires a publication HTTP(S) URL for its origin/path scope.');}
      if(!['http:','https:'].includes(url.protocol))throw new Error('SharedObject persistence requires an HTTP(S) origin; file and opaque origins are not supported.');
      if(secure&&url.protocol!=='https:')return null;
      const publication=url.pathname;let path=publication;
      if(localPath!==null&&localPath!==undefined){
        if(typeof localPath!=='string'||localPath.length>4096||!localPath.startsWith('/')||localPath.startsWith('//')||/[\\?#\x00-\x1f\x7f]/.test(localPath))throw new TypeError('SharedObject localPath requires an absolute publication path without query/fragment.');
        if(!completeUnicode(localPath)||/%(?![0-9a-f]{2})/i.test(localPath)||localPath.split('/').some(part=>/^\.{1,2}$/.test(part.replace(/%2e/ig,'.'))))throw new TypeError('SharedObject localPath requires complete Unicode and must not contain malformed escapes or dot path components.');
        const candidate=new URL(localPath,url.origin);path=candidate.pathname;
        if(path.length>1)path=path.replace(/\/+$/,'');
        if(path!=='/'&&publication!==path&&!publication.startsWith(path+'/'))throw new RangeError('SharedObject localPath must be the publication path or one of its ancestor directories.');
      }
      return PREFIX+JSON.stringify([url.origin,path,name,secure]);
    }
    function bytes(record){return amf.encode(record.data,record.encoding,{ignoreRootDynamicWriter:true});}
    function unpack(text,as2=false){
      if(typeof text!=='string'||text.length>Math.ceil(MAX_BYTES/3)*4+256)throw new RangeError('Saved SharedObject exceeds the 16 MiB AMF envelope limit.');
      let envelope;try{envelope=JSON.parse(text);}catch(error){throw new Error('Saved SharedObject envelope is malformed; clear this named record to recover.',{cause:error});}
      if(!envelope||envelope.version!==1||!Object.keys(envelope).every(key=>['version','encoding','bytes'].includes(key)))throw new Error('Saved SharedObject has an unsupported envelope version or fields.');
      encoding(envelope.encoding);const payload=amf.unbase64(envelope.bytes),result=amf.decode(payload,envelope.encoding,{complete:true,record:true,legacyXML:as2});
      if(result.consumed!==payload.length)throw new Error('Saved SharedObject has trailing AMF data.');
      if(result.value===null||typeof result.value!=='object'||Array.isArray(result.value)||Object.getPrototypeOf(result.value)!==Object.prototype)throw new Error('Saved SharedObject root must be an ordinary data record.');
      return {data:result.value,encoding:envelope.encoding};
    }
    function remoteCacheSuffix(persistence){
      return typeof persistence==='string' ? ':'+JSON.stringify(persistence) : '';
    }
    function getLocal(name,localPath=null,secure=false,as2=false){
      open();const key=location(name,localPath,secure);if(key===null)return null;
      let record=records.get(key);
      if(!record){
        if(records.size>=MAX_OBJECTS)throw new RangeError('At most 256 local SharedObjects may be opened in a publication session.');
        const text=access(store=>store.getItem(key)),loaded=text===null?{data:{},encoding:as2?0:defaultEncoding}:unpack(text,as2);
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
      if(!rec||network.isNetwork(rec))return;
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
    function commitSol(record,decoded,as2){
      const mode=encoding(decoded.encoding),value=decoded.value;
      if(!value||typeof value!=='object'||Array.isArray(value)||Object.getPrototypeOf(value)!==Object.prototype)throw new TypeError('SOL root must be an ordinary data record.');
      if(as2){
        const incoming=Object.getOwnPropertyDescriptors(value),previous=Object.getOwnPropertyDescriptors(record.data),keys=Reflect.ownKeys(incoming);
        // This data object is publication-owned. Preflight every operation before
        // changing it, and define fields without invoking setters or __proto__.
        for(const key of Reflect.ownKeys(previous))if(!previous[key].configurable)throw new TypeError('Cannot import into AS2 SharedObject.data with nonconfigurable fields.');
        if(!Object.isExtensible(record.data)&&keys.some(key=>!Object.hasOwn(previous,key)))throw new TypeError('Cannot add imported fields to nonextensible AS2 SharedObject.data.');
        for(const key of keys)if(!Object.hasOwn(incoming[key],'value'))throw new TypeError('SOL root requires data properties.');
        for(const key of keys)Object.defineProperty(record.data,key,{value:incoming[key].value,writable:true,enumerable:true,configurable:true});
        for(const key of Reflect.ownKeys(previous))if(!Object.hasOwn(incoming,key))delete record.data[key];
      }else record.data=value;
      record.encoding=mode;record.epoch++;
    }
    function importRecordSol(record,solBytes,as2=false){
      if(typeof amf.decodeSol!=='function')throw new Error('AMF service does not support .sol decoding.');
      if(record.busy)throw new Error('Recursive SharedObject serialization is not supported.');
      record.busy=true;const epoch=record.epoch,session=generation;
      try{
        return amf.decodeSol(solBytes,{legacyXML:as2,accept(decoded){
          if(disposed||generation!==session||record.generation!==generation||record.epoch!==epoch)throw new Error('SharedObject changed or was reset during SOL import.');
          commitSol(record,decoded,as2);
        }});
      }finally{record.busy=false;}
    }
    function fromSolBytes(solBytes,localPath,secure,as2){
      open();if(location('SharedObject',localPath,secure)===null)return null;
      const session=generation;let key,existing,epoch,locked=false,so;
      try{
        amf.decodeSol(solBytes,{legacyXML:as2,validateName(name){
          key=location(name,localPath,secure);existing=records.get(key);epoch=existing?.epoch;
          if(existing?.busy)throw new Error('Recursive SharedObject serialization is not supported.');
          if(existing){existing.busy=true;locked=true;}
        },accept(decoded){
          if(disposed||generation!==session||records.get(key)!==existing||existing?.epoch!==epoch)throw new Error('SharedObject changed or was reset during SOL import.');
          so=getLocal(decoded.name,localPath,secure,as2);
          if(disposed||generation!==session)throw new Error('SharedObject was reset during SOL import.');
          commitSol(state(so,as2),decoded,as2);
        }});
        return so;
      }finally{if(locked)existing.busy=false;}
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
    function remoteData(peer,rec){
        if(!rec.proxies)rec.proxies=new WeakMap();
        if(!rec.proxies.has(peer))rec.proxies.set(peer,new Proxy(rec.data,{
          set:(obj,key,value)=>{
            open();if(rec.generation!==generation)throw new Error('This remote SharedObject belongs to a previous session.');
            if(rec.network||rec.cache)network.update(peer,rec,key,value,false);
            else Object.defineProperty(obj,key,{value,enumerable:true,writable:true,configurable:true});
            return true;
          },
          deleteProperty:(obj,key)=>{
            open();if(rec.generation!==generation)throw new Error('This remote SharedObject belongs to a previous session.');
            if(!rec.network&&!rec.cache)return Reflect.deleteProperty(obj,key);
            network.update(peer,rec,key,undefined,true);
            return true;
          },
        }));
        return rec.proxies.get(peer);
    }
    class SharedObject extends events.EventDispatcher {
      constructor(secret,record){super();if(secret!==token)unsupported('SharedObject construction; use getLocal');states.set(this,record);Object.assign(this,hub.methods(this));Object.seal(this);}
      static getLocal(name,localPath=null,secure=false){return getLocal(name,localPath,secure,false);}
      static fromSolBytes(solBytes,localPath=null,secure=false){
        return fromSolBytes(solBytes,localPath,secure,false);
      }
      static get defaultObjectEncoding(){open();return defaultEncoding;}
      static set defaultObjectEncoding(value){open();defaultEncoding=encoding(value);}
      get data(){const rec=state(this);return rec.cache?remoteData(this,rec):rec.data;}
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
        try{return fromSolBytes(solBytes,localPath,secure,true);}catch(error){warn(error);return null;}
      }
      static getRemote(name,remotePath=null,persistence=false,secure=false){
        try{
          open();const check=location(name,null,Boolean(secure));if(check===null)return null;
          if(remotePath!==null&&remotePath!==undefined&&(typeof remotePath!=='string'||remotePath.length>4096||/[\x00-\x1f\x7f]/.test(remotePath)))throw new TypeError('SharedObject.getRemote remotePath requires a valid URI/path String or null.');
          persistence=persistence??false;
          const key=`remote:${remotePath??''}:${name}:${Boolean(secure)}`+remoteCacheSuffix(persistence);
          let record=remoteRecords.get(key);
          if(!record){
            if(remoteRecords.size>=MAX_OBJECTS)throw new RangeError('At most 256 remote SharedObjects may be opened in a publication session.');
            record={key,name,remotePath,secure,data:{},encoding:0,generation,epoch:0,busy:false,remote:true,persistence,fps:0,peers:new Set(),clients:new WeakMap()};
            network.openCache(record,remotePath);
            remoteRecords.set(key,record);
          }
          if(record.persistence!==persistence)throw new Error('Room subscribers must use the same persistence setting.');
          const instance=new LegacySharedObject(token,record);
          return instance;
        }catch(error){warn(error);return null;}
      }
      get data(){const rec=state(this,true);return rec.remote?remoteData(this,rec):rec.data;}
      getSize(){return bytes(state(this,true)).length;}
      exportSolBytes(){return exportRecordSol(state(this,true));}
      importSolBytes(solBytes){importRecordSol(state(this,true),solBytes,true);return this;}
      flush(minDiskSpace=0){
        const record=state(this,true);
        try{
          if(record.remote){if(!Number.isInteger(minDiskSpace)||minDiskSpace<0||minDiskSpace>MAX_BYTES)throw new RangeError('Invalid minDiskSpace.');if(network.isNetwork(record)){const result=network.flush(this,record);return result==='flushed'?true:result;}bytes(record);broadcastSync(record,Object.keys(record.data).map(k=>({code:'change',name:k})));return true;}
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
        if(network.isNetwork(record)){network.clear(this,record);return;}
        if(record.remote){for(const key of Reflect.ownKeys(record.data))delete record.data[key];record.epoch++;broadcastSync(record,[{code:'clear'}]);return;}
        clearRecord(record,true);
      }
      connect(nc){
        const record=state(this,true);
        if(!record.remote)throw new TypeError('SharedObject.connect requires a remote SharedObject from SharedObject.getRemote.');
        if(!nc||typeof nc!=='object'||!('isConnected'in nc||'uri'in nc))return false;
        try{if(network.connect(this,record,nc))return true;}catch(e){warn(e);return false;}
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
        if(network.isNetwork(record)){network.dirty(this,record,propertyName);return;}
        if(record.remote&&record.peers.has(this)&&typeof propertyName==='string'&&propertyName){
          const born=generation,targets=[...record.peers];
          broadcastSync(record,[{code:'change',name:propertyName}]);
          queueMicrotask(()=>{if(disposed||born!==generation)return;for(const peer of targets){if(!record.peers.has(peer))continue;try{if(typeof peer.onSync==='function')Reflect.apply(peer.onSync,peer,[[{code:'change',name:propertyName}]]);}catch(error){warn(error);}}});
        }
      }
      send(handlerName,...args){
        const record=state(this,true);
        if(network.isNetwork(record)){network.send(this,record,handlerName,args);return;}
        if(!record.remote||!record.peers.has(this))throw new Error('SharedObject.send requires a connected remote SharedObject.');
        if(typeof handlerName!=='string'||!handlerName||['__proto__','prototype','constructor'].includes(handlerName))throw new TypeError('SharedObject.send requires a valid handler name String.');
        const born=generation,targets=[...record.peers];
        broadcastSync(record,[],{name:handlerName,args});
        queueMicrotask(()=>{if(disposed||born!==generation)return;for(const peer of targets){if(!record.peers.has(peer))continue;try{const fn=peer[handlerName];if(typeof fn==='function')Reflect.apply(fn,peer,args);}catch(error){warn(error);}}});
      }
      close(){
        const record=state(this,true);
        if(network.isNetwork(record)){network.close(this);return;}
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
      persistence=persistence??false;
      const key=`as3remote:${remotePath??''}:${name}:${secure}`+remoteCacheSuffix(persistence);
      let record=remoteRecords.get(key);
      if(!record){
        if(remoteRecords.size>=MAX_OBJECTS)throw new RangeError('At most 256 remote SharedObjects may be opened in a publication session.');
        record={key,name,remotePath,data:{},encoding:defaultEncoding,generation,epoch:0,busy:false,remote:true,persistence,secure,fps:0,peers:new Set(),dirty:new Set(),clients:new WeakMap()};
        network.openCache(record,remotePath);
        remoteRecords.set(key,record);
      }
      if(record.persistence!==persistence)throw new Error('Room subscribers must use the same persistence setting.');
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
    const _as3Hook=Object.freeze({network,setNetworkEvents(sync,status){networkSync=sync;networkStatus=status;},createRemoteModern,remoteRecords,state:v=>state(v,false),bytes,amf,encoding,hub,events,warn,MAX_BYTES,isDisposed:()=>disposed,getGeneration:()=>generation,broadcastSync,handleRemoteSyncMessage,makeSyncEvent});
    function flushAll(){open();const failures=[];for(const record of records.values()){try{flushRecord(record);}catch(error){failures.push(error);warn(error);}}return failures;}
    function reset(){open();network.reset();generation++;records.clear();remoteRecords.clear();hub.clear();defaultEncoding=3;dynamicWriter=null;}
    function dispose(){if(disposed)return;reset();try{syncChannel?.close?.();}catch(_){}syncChannel=null;disposed=true;}
    return Object.freeze({SharedObject,LegacySharedObject,ObjectEncoding,SharedObjectFlushStatus,flushAll,reset,dispose,_as3Hook});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashStorage=api;
})(globalThis);
